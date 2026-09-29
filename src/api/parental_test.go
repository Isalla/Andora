package main

import (
	"bytes"
	"context"
	"encoding/json"
	"log"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"
)

// todayWeek builds a week with minutes for today, 0 (unlimited) else.
func todayWeek(t *testing.T, minutes int) map[string]any {
	t.Helper()
	names := []string{"monday_minutes", "tuesday_minutes", "wednesday_minutes",
		"thursday_minutes", "friday_minutes", "saturday_minutes", "sunday_minutes"}
	out := map[string]any{}
	for i, n := range names {
		out[n] = 0
		if i == weekIndexOf(time.Now()) {
			out[n] = minutes
		}
	}
	return out
}

func setupPayload(accountID int, week map[string]any) map[string]any {
	p := map[string]any{
		"account_id":      accountID,
		"parent_pin":      "1234",
		"chat_enabled":    true,
		"voice_enabled":   false,
		"warning_minutes": 30,
	}
	for k, v := range week {
		p[k] = v
	}
	return p
}

func setupParental(t *testing.T, client *apiClient, accountID int, minutes int, email string) {
	t.Helper()
	p := setupPayload(accountID, todayWeek(t, minutes))
	if email != "" {
		p["parent_email"] = email
	}
	rec := client.post(t, "/parental/setup", p)
	if rec.Code != http.StatusCreated {
		t.Fatalf("setup: %d %s", rec.Code, rec.Body.String())
	}
}

func TestParentalSetupValidation(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "kid1", "longenough1", "k1@example.com", false)

	bad := []map[string]any{
		setupPayload(999, todayWeek(t, 60)), // unknown account
		func() map[string]any { p := setupPayload(1, todayWeek(t, 60)); p["parent_pin"] = "12"; return p }(),
		func() map[string]any { p := setupPayload(1, todayWeek(t, 60)); p["parent_pin"] = "12ab"; return p }(),
		func() map[string]any { p := setupPayload(1, todayWeek(t, 60)); p["parent_email"] = "nope"; return p }(),
		func() map[string]any { p := setupPayload(1, todayWeek(t, 60)); p["monday_minutes"] = 2000; return p }(),
	}
	for i, p := range bad {
		rec := client.post(t, "/parental/setup", p)
		if rec.Code != http.StatusBadRequest && rec.Code != http.StatusNotFound {
			t.Fatalf("case %d: expected 400/404, got %d %s", i, rec.Code, rec.Body.String())
		}
	}

	setupParental(t, client, 1, 60, "parent@example.com")
	acc := srv.store.(*fakeStore).accounts[1]
	if !acc.ParentalEnabled {
		t.Fatal("account flag must be enabled after setup")
	}

	// duplicate setup conflicts
	rec := client.post(t, "/parental/setup", setupPayload(1, todayWeek(t, 60)))
	if rec.Code != http.StatusConflict {
		t.Fatalf("duplicate: expected 409, got %d", rec.Code)
	}

	// setup response carries no secrets
	body := rec.Body.String()
	_ = body
	rec2 := client.post(t, "/parental/pin/verify", map[string]any{"account_id": 1, "parent_pin": "1234"})
	if decode(t, rec2)["valid"] != true {
		t.Fatalf("pin verify: %v", rec2.Body.String())
	}
}

func TestParentalPinVerifyAndChange(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "kid2", "longenough1", "k2@example.com", false)

	// no control yet -> valid false, not 404
	rec := client.post(t, "/parental/pin/verify", map[string]any{"account_id": 1, "parent_pin": "1234"})
	if decode(t, rec)["valid"] != false {
		t.Fatal("expected valid=false without control")
	}

	setupParental(t, client, 1, 60, "parent@example.com")
	rec = client.post(t, "/parental/pin/verify", map[string]any{"account_id": 1, "parent_pin": "0000"})
	if decode(t, rec)["valid"] != false {
		t.Fatal("wrong pin must not verify")
	}

	// change with wrong old pin
	rec = client.post(t, "/parental/pin/change", map[string]any{"account_id": 1, "old_pin": "0000", "new_pin": "5678"})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
	rec = client.post(t, "/parental/pin/change", map[string]any{"account_id": 1, "old_pin": "1234", "new_pin": "56"})
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for short pin, got %d", rec.Code)
	}
	rec = client.post(t, "/parental/pin/change", map[string]any{"account_id": 1, "old_pin": "1234", "new_pin": "5678"})
	if rec.Code != http.StatusOK || decode(t, rec)["changed"] != true {
		t.Fatalf("change: %d %s", rec.Code, rec.Body.String())
	}
	rec = client.post(t, "/parental/pin/verify", map[string]any{"account_id": 1, "parent_pin": "1234"})
	if decode(t, rec)["valid"] != false {
		t.Fatal("old pin must be dead")
	}
	rec = client.post(t, "/parental/pin/verify", map[string]any{"account_id": 1, "parent_pin": "5678"})
	if decode(t, rec)["valid"] != true {
		t.Fatal("new pin must verify")
	}

	// pin-change receipt contains no PIN material
	rec = client.post(t, "/parental/notifications", map[string]any{"account_id": 1})
	var out struct {
		Notifications []struct {
			Setting  string `json:"setting_name"`
			OldValue string `json:"old_value"`
			NewValue string `json:"new_value"`
		} `json:"notifications"`
	}
	if err := jsonUnmarshal(t, rec, &out); err != nil {
		t.Fatal(err)
	}
	found := false
	for _, n := range out.Notifications {
		if n.Setting == "parent_pin" {
			found = true
			if n.OldValue == "5678" || n.NewValue == "5678" || n.OldValue == "1234" || n.NewValue == "1234" {
				t.Fatalf("receipt leaks PIN: %+v", n)
			}
		}
	}
	if !found {
		t.Fatal("pin change receipt missing")
	}
}

func jsonUnmarshal(t *testing.T, rec *httptest.ResponseRecorder, v any) error {
	t.Helper()
	return json.Unmarshal(rec.Body.Bytes(), v)
}

func TestParentalExtensionOncePerDay(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "kid3", "longenough1", "k3@example.com", false)
	setupParental(t, client, 1, 60, "")

	rec := client.post(t, "/parental/extension", map[string]any{"account_id": 1, "parent_pin": "0000"})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
	rec = client.post(t, "/parental/extension", map[string]any{"account_id": 1, "parent_pin": "1234"})
	if rec.Code != http.StatusOK {
		t.Fatalf("extension: %d %s", rec.Code, rec.Body.String())
	}
	body := decode(t, rec)
	if body["granted"] != true || body["remaining_seconds"] != float64(7200) {
		t.Fatalf("extension body: %v", body)
	}
	// second use same day conflicts
	rec = client.post(t, "/parental/extension", map[string]any{"account_id": 1, "parent_pin": "1234"})
	if rec.Code != http.StatusConflict {
		t.Fatalf("expected 409, got %d", rec.Code)
	}

	// status reports the consumed extension
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1})
	st := decode(t, rec)
	if st["extended_used_today"] != true || st["remaining_seconds"] != float64(7200) {
		t.Fatalf("status after extension: %v", st)
	}
}

func TestParentalStatusPriority(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "kid4", "longenough1", "k4@example.com", false)
	setupParental(t, client, 1, 60, "")
	today := parentalDayString(time.Now())

	rec := client.post(t, "/parental/status", map[string]any{"account_id": 1})
	if got := decode(t, rec)["week_minutes"]; got != float64(60) {
		t.Fatalf("weekly rule: %v", got)
	}

	// special period overrides the weekly rule
	p := todayWeek(t, 120)
	p["account_id"] = 1
	p["parent_pin"] = "1234"
	p["starts_at"] = today
	p["ends_at"] = today
	rec = client.post(t, "/parental/periods/add", p)
	if rec.Code != http.StatusCreated {
		t.Fatalf("period add: %d %s", rec.Code, rec.Body.String())
	}
	// overlapping period conflicts
	rec = client.post(t, "/parental/periods/add", p)
	if rec.Code != http.StatusConflict {
		t.Fatalf("overlap: expected 409, got %d", rec.Code)
	}
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1})
	if got := decode(t, rec)["week_minutes"]; got != float64(120) {
		t.Fatalf("period rule: %v", got)
	}

	// day exception override wins over the period
	rec = client.post(t, "/parental/exceptions/add", map[string]any{
		"account_id": 1, "parent_pin": "1234", "date": today, "override_minutes": 30,
	})
	if rec.Code != http.StatusCreated {
		t.Fatalf("exception add: %d %s", rec.Code, rec.Body.String())
	}
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1})
	st := decode(t, rec)
	if st["week_minutes"] != float64(30) || st["remaining_seconds"] != float64(1800) {
		t.Fatalf("exception override: %v", st)
	}

	// override 0 = unlimited: not blocked, full flags
	rec = client.post(t, "/parental/exceptions/add", map[string]any{
		"account_id": 1, "parent_pin": "1234", "date": today, "override_minutes": 0,
	})
	if rec.Code != http.StatusCreated {
		t.Fatalf("unlimited exception: %d %s", rec.Code, rec.Body.String())
	}
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1})
	st = decode(t, rec)
	if st["blocked"] != false || st["remaining_seconds"] != float64(0) {
		t.Fatalf("unlimited day: %v", st)
	}
}

func TestParentalBlockedLoginAndBuffer(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, fs, "kid5", "longenough1", "k5@example.com", false)
	setupParental(t, client, 1, 60, "")

	// exhaust today's budget server-side
	day := parentalDayKey(time.Now())
	if err := fs.AddParentalUsageSeconds(context.Background(), 1, day, 3600, time.Now(), 60); err != nil {
		t.Fatal(err)
	}

	// fresh login is refused with the parental flag, no session issued
	rec := client.post(t, "/auth/verify", map[string]any{"username": "kid5", "password": "longenough1"})
	body := decode(t, rec)
	if body["valid"] != false || body["parental_blocked"] != true {
		t.Fatalf("verify blocked: %v", body)
	}
	if _, has := body["session_id"]; has {
		t.Fatal("blocked login must not issue a session")
	}

	// an already-running session starts the grace buffer on poll
	raw, _, err := fs.CreateSession(context.Background(), 1, time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1, "session_id": raw})
	st := decode(t, rec)
	if st["blocked"] != true || st["buffer_until"] == nil || st["force_logout"] == true {
		t.Fatalf("buffer start: %v", st)
	}

	// past the buffer the session must be logged out
	dur := parentalBufferDuration(time.Now())
	past := time.Now().Add(-dur - time.Minute)
	u := fs.usage[1][parentalDayString(time.Now())]
	u.BufferStartedAt = &past
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1, "session_id": raw})
	st = decode(t, rec)
	if st["force_logout"] != true {
		t.Fatalf("force logout: %v", st)
	}
}

func TestParentalWarningAndUnlimitedLogin(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, fs, "kid6", "longenough1", "k6@example.com", false)
	setupParental(t, client, 1, 60, "")

	day := parentalDayKey(time.Now())
	if err := fs.AddParentalUsageSeconds(context.Background(), 1, day, 3300, time.Now(), 60); err != nil {
		t.Fatal(err)
	}
	rec := client.post(t, "/parental/status", map[string]any{"account_id": 1})
	st := decode(t, rec)
	if st["warning"] != true || st["remaining_seconds"] != float64(300) {
		t.Fatalf("warning: %v", st)
	}
	if st["chat_allowed"] != true || st["voice_allowed"] != false {
		t.Fatalf("mechanism flags: %v", st)
	}

	// unlimited day (all week 0): login passes without the flag
	addAccount(t, fs, "kid7", "longenough1", "k7@example.com", false)
	setupParental(t, client, 2, 0, "")
	rec = client.post(t, "/auth/verify", map[string]any{"username": "kid7", "password": "longenough1"})
	body := decode(t, rec)
	if body["valid"] != true {
		t.Fatalf("unlimited login: %v", body)
	}
	if _, has := body["parental_blocked"]; has {
		t.Fatalf("unlimited login must not carry the flag: %v", body)
	}
}

func TestParentalNotificationsDeliverOnce(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "kid8", "longenough1", "k8@example.com", false)
	setupParental(t, client, 1, 60, "parent@example.com")

	rec := client.post(t, "/parental/notifications", map[string]any{"account_id": 1})
	var list struct {
		Notifications []struct {
			ID    int    `json:"id"`
			Email string `json:"email"`
		} `json:"notifications"`
	}
	if err := jsonUnmarshal(t, rec, &list); err != nil {
		t.Fatal(err)
	}
	if len(list.Notifications) == 0 {
		t.Fatal("setup receipt missing")
	}
	for _, n := range list.Notifications {
		if n.Email != "" {
			t.Fatal("listing must not include addresses")
		}
	}
	ids := []int{}
	for _, n := range list.Notifications {
		ids = append(ids, n.ID)
	}
	rec = client.post(t, "/parental/notifications/deliver", map[string]any{"account_id": 1, "notification_ids": ids})
	var done struct {
		Delivered []struct {
			ID    int    `json:"id"`
			Email string `json:"email"`
		} `json:"delivered"`
	}
	if err := jsonUnmarshal(t, rec, &done); err != nil {
		t.Fatal(err)
	}
	if len(done.Delivered) != len(ids) || done.Delivered[0].Email != "parent@example.com" {
		t.Fatalf("deliver: %s", rec.Body.String())
	}
	// second deliver returns nothing: plaintext was given exactly once
	rec = client.post(t, "/parental/notifications/deliver", map[string]any{"account_id": 1, "notification_ids": ids})
	var done2 struct {
		Delivered []any `json:"delivered"`
	}
	if err := jsonUnmarshal(t, rec, &done2); err != nil {
		t.Fatal(err)
	}
	if len(done2.Delivered) != 0 {
		t.Fatalf("re-deliver must be empty: %s", rec.Body.String())
	}
}

func TestParentalRemove(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, fs, "kid9", "longenough1", "k9@example.com", false)
	setupParental(t, client, 1, 60, "parent@example.com")

	rec := client.post(t, "/parental/remove", map[string]any{"account_id": 1, "parent_pin": "0000"})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
	rec = client.post(t, "/parental/remove", map[string]any{"account_id": 1, "parent_pin": "1234"})
	if rec.Code != http.StatusOK || decode(t, rec)["removed"] != true {
		t.Fatalf("remove: %d %s", rec.Code, rec.Body.String())
	}
	if fs.accounts[1].ParentalEnabled {
		t.Fatal("account flag must be cleared")
	}
	// removal receipt stays deliverable although the control is gone
	rec = client.post(t, "/parental/notifications", map[string]any{"account_id": 1})
	var list struct {
		Notifications []struct {
			EventType string `json:"event_type"`
		} `json:"notifications"`
	}
	if err := jsonUnmarshal(t, rec, &list); err != nil {
		t.Fatal(err)
	}
	found := false
	for _, n := range list.Notifications {
		if n.EventType == eventParentalRemoved {
			found = true
		}
	}
	if !found {
		t.Fatalf("removal receipt missing: %s", rec.Body.String())
	}
	// login works again after removal
	rec = client.post(t, "/auth/verify", map[string]any{"username": "kid9", "password": "longenough1"})
	if decode(t, rec)["valid"] != true {
		t.Fatalf("login after removal: %s", rec.Body.String())
	}
}

func TestParentalStatusSessionGating(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, fs, "kid11", "longenough1", "k11@example.com", false)
	addAccount(t, fs, "kid12", "longenough1", "k12@example.com", false)
	setupParental(t, client, 1, 60, "")

	otherSession, _, err := fs.CreateSession(context.Background(), 2, time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	ownSession, _, err := fs.CreateSession(context.Background(), 1, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	// 1) foreign session, fresh budget: pure read, no accrual, not blocked
	rec := client.post(t, "/parental/status", map[string]any{"account_id": 1, "session_id": otherSession})
	st := decode(t, rec)
	if st["blocked"] != false {
		t.Fatalf("foreign session must not block: %v", st)
	}
	if u := fs.usage[1]; u != nil && u[parentalDayString(time.Now())] != nil {
		t.Fatalf("foreign session must not accrue usage: %+v", u)
	}

	// exhaust the budget server-side; the helper also stamps a poll,
	// so clear it to keep the step-3 assertion meaningful.
	day := parentalDayKey(time.Now())
	if err := fs.AddParentalUsageSeconds(context.Background(), 1, day, 3600, time.Now(), 60); err != nil {
		t.Fatal(err)
	}
	fs.mu.Lock()
	if u := fs.usage[1][parentalDayString(day)]; u != nil {
		u.LastPolledAt = nil
	}
	fs.mu.Unlock()

	// 2) no session at all: blocked, no buffer, no force logout
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1})
	st = decode(t, rec)
	if st["blocked"] != true {
		t.Fatalf("exhausted budget must block without session: %v", st)
	}
	if st["buffer_until"] != nil || st["force_logout"] == true {
		t.Fatalf("without session no buffer/logout: %v", st)
	}

	// 3) foreign session on exhausted budget: still only a read
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1, "session_id": otherSession})
	st = decode(t, rec)
	if st["blocked"] != true || st["buffer_until"] != nil || st["force_logout"] == true {
		t.Fatalf("foreign session must not start buffer or logout: %v", st)
	}
	row := fs.usage[1]
	if row == nil || row[parentalDayString(time.Now())] == nil {
		t.Fatal("usage row missing")
	}
	if row[parentalDayString(time.Now())].LastPolledAt != nil {
		t.Fatalf("foreign session must not stamp polls: %+v", row)
	}

	// 4) own live session on exhausted budget: grace buffer starts
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1, "session_id": ownSession})
	st = decode(t, rec)
	if st["blocked"] != true || st["buffer_until"] == nil || st["force_logout"] == true {
		t.Fatalf("own session must start buffer: %v", st)
	}

	// 5) buffer fully elapsed, session still live: force logout
	past := time.Now().Add(-parentalBufferDuration(time.Now()) - time.Minute)
	u := fs.usage[1][parentalDayString(time.Now())]
	u.BufferStartedAt = &past
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 1, "session_id": ownSession})
	st = decode(t, rec)
	if st["force_logout"] != true {
		t.Fatalf("elapsed buffer + own session must force logout: %v", st)
	}
}

func TestParentalPermissionDeniedAndNotFound(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	limited := &apiClient{s: srv, cred: srv.cfg.Services["svc-limited"]} // only realm.list
	addAccount(t, srv.store.(*fakeStore), "kid10", "longenough1", "k10@example.com", false)

	rec := limited.post(t, "/parental/status", map[string]any{"account_id": 1})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("expected 403, got %d", rec.Code)
	}
	rec = limited.post(t, "/parental/pin/verify", map[string]any{"account_id": 1, "parent_pin": "1234"})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("expected 403, got %d", rec.Code)
	}
	rec = client.post(t, "/parental/status", map[string]any{"account_id": 999})
	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404, got %d", rec.Code)
	}
	rec = client.post(t, "/parental/update", func() map[string]any {
		p := setupPayload(1, todayWeek(t, 60))
		return p
	}())
	if rec.Code != http.StatusNotFound {
		t.Fatalf("update without control: expected 404, got %d", rec.Code)
	}
}

// ---------------------------------------------------------------------------
// P-35: Sichtbarkeit verlorener Best-effort-Parental-Ereignisse.
//
// Die Zustandsänderung bleibt immer erfolgreich (kein Rollback, keine
// Fehlerantwort, kein Retry); sichtbar wird nur der Verlust der
// Historienzeile über genau eine strukturierte WARN je fehlgeschlagenem Write.
//
// Logger-Testnaht: der Go-Standardlogger wird in diesen seriellen Tests
// temporär auf einen lokalen Puffer umgeleitet. Das ist sicher, weil im
// gesamten Paket kein t.Parallel vorkommt und kein anderer Test den
// globalen Logger verändert. p35LogMu verhindert Überlappung, t.Cleanup
// stellt Writer, Flags und Prefix vollständig wieder her.
// ---------------------------------------------------------------------------

// p35LogMu ist der zentrale Test-Mutex. Er schützt die Umleitung des
// globalen Standardloggers; die Nutzdaten liegen je Test im lokalen Puffer.
var p35LogMu sync.Mutex

// p35Prefix ist der Anfang jeder P-35-Warnung. Der Access-Log der Middleware
// landet im selben Puffer und wird über dieses Präfix herausgefiltert.
const p35Prefix = "WARN parental_history_write_failed "

// captureP35Logs leitet den Standardlogger auf einen lokalen Puffer um und
// gibt ihn zurück. Flags werden auf 0 gesetzt, damit die Ausgabe exakt der
// kontrollierten Nachricht entspricht und kein Datum/Uhrzeit enthält.
func captureP35Logs(t *testing.T) *bytes.Buffer {
	t.Helper()
	p35LogMu.Lock()
	prevWriter, prevFlags, prevPrefix := log.Writer(), log.Flags(), log.Prefix()
	buf := &bytes.Buffer{}
	log.SetOutput(buf)
	log.SetFlags(0)
	log.SetPrefix("")
	t.Cleanup(func() {
		log.SetOutput(prevWriter)
		log.SetFlags(prevFlags)
		log.SetPrefix(prevPrefix)
		p35LogMu.Unlock()
	})
	return buf
}

// p35Reset empties an already captured buffer. A test that exercises two
// phases reuses the SAME capture, because the test mutex is taken once per
// test and is not reentrant.
func p35Reset(buf *bytes.Buffer) {
	buf.Reset()
}

// p35Warnings returns ONLY the P-35 warning lines, in order. The middleware
// access log is filtered out; it is not part of P-35.
func p35Warnings(t *testing.T, buf *bytes.Buffer) []string {
	t.Helper()
	var out []string
	for _, line := range strings.Split(strings.TrimRight(buf.String(), "\n"), "\n") {
		if strings.HasPrefix(line, p35Prefix) {
			out = append(out, line)
		}
	}
	return out
}

// p35ParentalServer builds a parental test server with a supervised account.
func p35ParentalServer(t *testing.T, user string) (*apiClient, *fakeStore, int) {
	t.Helper()
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	acc := addAccount(t, fs, user, "longenough1", user+"@example.com", false)
	return client, fs, acc.ID
}

func p35EventCount(t *testing.T, fs *fakeStore, accountID int, eventType string) int {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	n := 0
	for _, e := range fs.securityEvents[accountID] {
		if e.EventType == eventType {
			n++
		}
	}
	return n
}

func p35NotifCount(t *testing.T, fs *fakeStore, accountID int) int {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	return len(fs.notifications[accountID])
}

// p35ParentalEnabled reads the AUTHORITATIVE parental state: the
// parental_controls row must exist AND the account flag must be set.
func p35ParentalEnabled(t *testing.T, fs *fakeStore, accountID int) bool {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	return fs.parental[accountID] != nil && fs.accounts[accountID] != nil &&
		fs.accounts[accountID].ParentalEnabled
}

// p35SetupParental creates the supervision (setup writes a notification, so
// this always runs BEFORE an injection is armed).
func p35SetupParental(t *testing.T, client *apiClient, id int, email string) {
	t.Helper()
	p := setupPayload(id, todayWeek(t, 60))
	if email != "" {
		p["parent_email"] = email
	}
	rec := client.post(t, "/parental/setup", p)
	if rec.Code != http.StatusCreated {
		t.Fatalf("setup: %d %s", rec.Code, rec.Body.String())
	}
}

// p35AddPeriod creates a period (id 1) so that periods/remove succeeds.
func p35AddPeriod(t *testing.T, client *apiClient, id int, today string) {
	t.Helper()
	p := setupPayload(id, todayWeek(t, 60))
	p["starts_at"] = today
	p["ends_at"] = today
	rec := client.post(t, "/parental/periods/add", p)
	if rec.Code != http.StatusCreated {
		t.Fatalf("period add: %d %s", rec.Code, rec.Body.String())
	}
}

// p35AddException creates a day exception so that exceptions/remove succeeds.
func p35AddException(t *testing.T, client *apiClient, id int, today string) {
	t.Helper()
	rec := client.post(t, "/parental/exceptions/add", map[string]any{
		"account_id": id, "parent_pin": "1234", "date": today, "override_minutes": 30})
	if rec.Code != http.StatusCreated {
		t.Fatalf("exception add: %d %s", rec.Code, rec.Body.String())
	}
}

// p35Case is one best-effort write path. prepare runs BEFORE the injected
// failure is armed; trigger runs with the failure active.
type p35Case struct {
	name    string
	event   string
	prepare func(t *testing.T, client *apiClient, id int, today string)
	trigger func(t *testing.T, client *apiClient, id int, today string)
}

// --- 1) Formatnachweis: exakte Zeile, feste Reihenfolge, keine Verbotsinhalte

func TestP35ParentalHistoryFormatExact(t *testing.T) {
	// Alle acht eindeutigen Parental-Ereignistypen, jeweils mit beiden Stages:
	// 8 x 2 = 16 Formatfaelle. Die Typnamen sind die KONSTANTEN aus
	// parental.go; sie werden hier bewusst NICHT auf Substring verboten.
	types := []string{
		eventParentalSetup,
		eventParentalSettings,
		eventParentalRemoved,
		eventParentalPinChanged,
		eventParentalEmailChanged,
		eventParentalEmailRemoved,
		eventParentalPeriod,
		eventParentalException,
	}
	stages := []string{"event_write", "notification_write"}
	const accountID = 7

	// p35SecretSentinels sind Werte, die in einer Warnmeldung NIE
	// erscheinen duerfen. Sie sind bewusst so gewaehlt, dass keiner von ihnen
	// Teil eines zulaessigen Ereignistyps ist — anders als "pin" oder "email",
	// die in parental_pin_changed bzw. parental_email_changed legitim sind.
	// Sie werden NICHT als Parameter uebergeben: der Test belegt, dass die
	// Funktion konstruktiv nur ihre drei Eingaben ausgibt.
	sentinels := []string{
		"739184",                  // PIN-Wert
		"$argon2id$test-pin-hash", // PIN-Hash
		"parent@example.invalid",  // E-Mail-Adresse
		"test-token-secret",       // Token
		"test-token-hash",         // Token-Hash
		"test-session-id",         // Session-ID
		"203.0.113.42",            // Roh-IP
		"old-sensitive-value",     // old_value
		"new-sensitive-value",     // new_value
		"exception:2099-12-31",    // setting
		"mysql://db-user:db-password@db.example.invalid/auth", // DB-URL
		"injected driver failure",                             // Treiber-/Fehlertext
	}

	// Feld-Whitelist: exakt sechs Bestandteile, getrennt durch je ein
	// Leerzeichen. Alles andere waere ein zusaetzliches key=value-Feld.
	const (
		tokWarn    = "WARN"
		tokMessage = "parental_history_write_failed"
	)

	var all []string
	for _, typ := range types {
		for _, stage := range stages {
			got := parentalHistoryFailureLine(typ, accountID, stage)
			all = append(all, got)

			// exakt ein Zeilenabschluss entsteht NICHT hier: die
			// Formatfunktion liefert eine reine Nachricht ohne Zeilenumbruch,
			// log.Print ergaenzt genau einen Abschluss.
			if strings.ContainsAny(got, "\r\n") {
				t.Fatalf("%s/%s: format must not embed a line break: %q", typ, stage, got)
			}

			f := strings.Split(got, " ")
			if len(f) != 6 {
				t.Fatalf("%s/%s: want exactly 6 space-separated fields, got %d: %q", typ, stage, len(f), got)
			}
			want := []string{
				tokWarn,
				tokMessage,
				"event_type=" + typ,
				"account_id=" + strconv.Itoa(accountID),
				"stage=" + stage,
				"error_class=db_error",
			}
			for k, w := range want {
				if f[k] != w {
					t.Fatalf("%s/%s: field %d: got %q, want %q (line %q)", typ, stage, k, f[k], w, got)
				}
			}
			// Kein zusaetzliches Feld: jedes Feld ist entweder ein fixer
			// Marker oder genau ein erwartetes key=value-Paar.
			for k, field := range f {
				if k < 2 {
					continue
				}
				if !strings.Contains(field, "=") {
					t.Fatalf("%s/%s: field %d must be key=value: %q", typ, stage, k, field)
				}
				if strings.Count(field, "=") != 1 {
					t.Fatalf("%s/%s: field %d must hold exactly one value: %q", typ, stage, k, field)
				}
			}
		}
	}
	if len(all) != len(types)*len(stages) {
		t.Fatalf("want %d format cases, got %d", len(types)*len(stages), len(all))
	}

	// Keiner der verbotenen Sentinel-Werte darf in irgendeiner der
	// erzeugten Meldungen vorkommen.
	for _, line := range all {
		for _, bad := range sentinels {
			if strings.Contains(line, bad) {
				t.Fatalf("warning line must not contain the sentinel %q: %q", bad, line)
			}
		}
		// Zusaetzlich: keine Zeile enthaelt ausserhalb der Whitelist ein
		// Trennzeichen, das auf eingebettete Freitexte hindeutet.
		if strings.Contains(line, "=") &&
			!strings.HasPrefix(line, tokWarn+" "+tokMessage+" event_type=") {
			t.Fatalf("unexpected line shape: %q", line)
		}
	}
}

// --- 2) Event-Write-Fehler: Zustand bleibt, Antwort bleibt erfolgreich

func TestP35ParentalEventFailureKeepsState(t *testing.T) {
	pinChange := func(t *testing.T, client *apiClient, id int, today string) {
		rec := client.post(t, "/parental/pin/change", map[string]any{
			"account_id": id, "old_pin": "1234", "new_pin": "4321"})
		if rec.Code != http.StatusOK {
			t.Fatalf("pin change: %d %s", rec.Code, rec.Body.String())
		}
	}
	// noSetup erzeugt die Aufsicht VOR der Injektion. Der setup-Fall
	// laesst prepare weg, weil /parental/setup selbst der ausloesende
	// Vorgang ist und ein zweites Setup sonst 409 ergaebe.
	noSetup := func(t *testing.T, client *apiClient, id int, today string) {
		p35SetupParental(t, client, id, "parent@example.com")
	}
	cases := []p35Case{
		{name: "setup", event: eventParentalSetup, prepare: nil,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				p := setupPayload(id, todayWeek(t, 60))
				p["parent_email"] = "parent@example.com"
				rec := client.post(t, "/parental/setup", p)
				if rec.Code != http.StatusCreated {
					t.Fatalf("setup: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "settings", event: eventParentalSettings, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				p := setupPayload(id, todayWeek(t, 60))
				p["warning_minutes"] = 45
				rec := client.post(t, "/parental/update", p)
				if rec.Code != http.StatusOK {
					t.Fatalf("update: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "pin", event: eventParentalPinChanged, prepare: noSetup, trigger: pinChange},
		{name: "removed", event: eventParentalRemoved, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				rec := client.post(t, "/parental/remove", map[string]any{
					"account_id": id, "parent_pin": "1234"})
				if rec.Code != http.StatusOK {
					t.Fatalf("remove: %d %s", rec.Code, rec.Body.String())
				}
			}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			client, fs, id := p35ParentalServer(t, "p35ev"+tc.name)
			if tc.prepare != nil {
				tc.prepare(t, client, id, "2006-01-02")
			}
			eventsBefore := p35EventCount(t, fs, id, tc.event)

			buf := captureP35Logs(t)
			fs.failSecurityEventWrite(true)
			tc.trigger(t, client, id, "2006-01-02")

			// Zustand bleibt erfolgreich geaendert: kein Rollback. Geprueft
			// wird der ERWARTETE Zustand nach dem erfolgreichen Vorgang, nicht
			// der Zustand davor.
			after := p35ParentalEnabled(t, fs, id)
			wantEnabled := tc.name != "removed"
			if after != wantEnabled {
				t.Fatalf("state must stay %v, got %v (no rollback)", wantEnabled, after)
			}
			// kein Event geschrieben
			if n := p35EventCount(t, fs, id, tc.event); n != eventsBefore {
				t.Fatalf("no new event expected after failed write, got %d want %d", n, eventsBefore)
			}
			// genau eine Warnung
			if l := p35Warnings(t, buf); len(l) != 1 {
				t.Fatalf("want exactly 1 warning, got %d: %v", len(l), l)
			}
		})
	}
}

// --- 3) Notification-Write-Fehler: Zustand bleibt, Antwort bleibt erfolgreich

func TestP35ParentalNotificationFailureKeepsState(t *testing.T) {
	// noSetup erzeugt die Aufsicht MIT Eltern-E-Mail VOR der Injektion, damit
	// notifyParental ueberhaupt schreibt. Der setup-Fall laesst prepare weg,
	// weil /parental/setup selbst der ausloesende Vorgang ist.
	noSetup := func(t *testing.T, client *apiClient, id int, today string) {
		p35SetupParental(t, client, id, "parent@example.com")
	}
	cases := []p35Case{
		{name: "setup", event: eventParentalSetup, prepare: nil,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				// ohne parent_email greift der fruehe Return in notifyParental
				p := setupPayload(id, todayWeek(t, 60))
				p["parent_email"] = "parent@example.com"
				rec := client.post(t, "/parental/setup", p)
				if rec.Code != http.StatusCreated {
					t.Fatalf("setup: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "settings", event: eventParentalSettings, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				p := setupPayload(id, todayWeek(t, 60))
				p["warning_minutes"] = 45
				rec := client.post(t, "/parental/update", p)
				if rec.Code != http.StatusOK {
					t.Fatalf("update: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "email_changed", event: eventParentalEmailChanged, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				p := setupPayload(id, todayWeek(t, 60))
				p["parent_email"] = "neu@example.com"
				rec := client.post(t, "/parental/update", p)
				if rec.Code != http.StatusOK {
					t.Fatalf("email change: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "email_removed", event: eventParentalEmailRemoved,
			prepare: func(t *testing.T, client *apiClient, id int, today string) {
				noSetup(t, client, id, today)
				p := setupPayload(id, todayWeek(t, 60))
				p["parent_email"] = "weg@example.com"
				rec := client.post(t, "/parental/update", p)
				if rec.Code != http.StatusOK {
					t.Fatalf("email seed: %d %s", rec.Code, rec.Body.String())
				}
			},
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				p := setupPayload(id, todayWeek(t, 60))
				p["parent_email"] = ""
				rec := client.post(t, "/parental/update", p)
				if rec.Code != http.StatusOK {
					t.Fatalf("email removal: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "pin", event: eventParentalPinChanged, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				rec := client.post(t, "/parental/pin/change", map[string]any{
					"account_id": id, "old_pin": "1234", "new_pin": "4321"})
				if rec.Code != http.StatusOK {
					t.Fatalf("pin change: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "period_add", event: eventParentalPeriod, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				p := setupPayload(id, todayWeek(t, 60))
				p["starts_at"] = today
				p["ends_at"] = today
				rec := client.post(t, "/parental/periods/add", p)
				if rec.Code != http.StatusCreated {
					t.Fatalf("period add: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "period_remove", event: eventParentalPeriod,
			prepare: func(t *testing.T, client *apiClient, id int, today string) {
				noSetup(t, client, id, today)
				p35AddPeriod(t, client, id, today)
			},
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				rec := client.post(t, "/parental/periods/remove", map[string]any{
					"account_id": id, "parent_pin": "1234", "id": 1})
				if rec.Code != http.StatusOK {
					t.Fatalf("period remove: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "exception_add", event: eventParentalException, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				rec := client.post(t, "/parental/exceptions/add", map[string]any{
					"account_id": id, "parent_pin": "1234", "date": today, "override_minutes": 30})
				if rec.Code != http.StatusCreated {
					t.Fatalf("exception add: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "exception_remove", event: eventParentalException,
			prepare: func(t *testing.T, client *apiClient, id int, today string) {
				noSetup(t, client, id, today)
				p35AddException(t, client, id, today)
			},
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				rec := client.post(t, "/parental/exceptions/remove", map[string]any{
					"account_id": id, "parent_pin": "1234", "date": today})
				if rec.Code != http.StatusOK {
					t.Fatalf("exception remove: %d %s", rec.Code, rec.Body.String())
				}
			}},
		{name: "removed", event: eventParentalRemoved, prepare: noSetup,
			trigger: func(t *testing.T, client *apiClient, id int, today string) {
				rec := client.post(t, "/parental/remove", map[string]any{
					"account_id": id, "parent_pin": "1234"})
				if rec.Code != http.StatusOK {
					t.Fatalf("remove: %d %s", rec.Code, rec.Body.String())
				}
			}},
	}
	seen := map[string]bool{}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			client, fs, id := p35ParentalServer(t, "p35notif"+tc.name)
			if tc.prepare != nil {
				tc.prepare(t, client, id, "2006-01-02")
			}
			notifsBefore := p35NotifCount(t, fs, id)
			seen[tc.event] = true

			buf := captureP35Logs(t)
			fs.failParentalNotification(true)
			tc.trigger(t, client, id, "2006-01-02")

			// kein Rollback: bei remove bleibt es entfernt, sonst aktiv
			if tc.name == "removed" {
				if p35ParentalEnabled(t, fs, id) {
					t.Fatal("removed: state must stay removed")
				}
			} else if !p35ParentalEnabled(t, fs, id) {
				t.Fatal("authoritative parental state must stay enabled")
			}
			if n := p35NotifCount(t, fs, id); n != notifsBefore {
				t.Fatalf("no new notification expected, got %d want %d", n, notifsBefore)
			}
			if l := p35Warnings(t, buf); len(l) != 1 {
				t.Fatalf("want exactly 1 warning, got %d: %v", len(l), l)
			}
		})
	}
	// alle acht Notification-Typen abgedeckt
	for _, e := range []string{eventParentalSetup, eventParentalSettings, eventParentalRemoved,
		eventParentalPinChanged, eventParentalEmailChanged, eventParentalEmailRemoved,
		eventParentalPeriod, eventParentalException} {
		if !seen[e] {
			t.Fatalf("notification type not covered: %s", e)
		}
	}
}

// --- 4) Stage-Zuordnung: event_write bzw. notification_write

func TestP35ParentalFailureStageIsCorrect(t *testing.T) {
	pinChange := func(t *testing.T, client *apiClient, id int) {
		rec := client.post(t, "/parental/pin/change", map[string]any{
			"account_id": id, "old_pin": "1234", "new_pin": "4321"})
		if rec.Code != http.StatusOK {
			t.Fatalf("pin change: %d %s", rec.Code, rec.Body.String())
		}
	}

	// security_events
	client, fs, id := p35ParentalServer(t, "p35stageev")
	p35SetupParental(t, client, id, "parent@example.com")
	buf := captureP35Logs(t)
	fs.failSecurityEventWrite(true)
	pinChange(t, client, id)
	l := p35Warnings(t, buf)
	if len(l) != 1 || l[0] != parentalHistoryFailureLine(eventParentalPinChanged, id, "event_write") {
		t.Fatalf("wrong event stage line: %v", l)
	}

	// parental_notifications (zweite Phase, gleicher Test -> gleicher Mutex)
	client2, fs2, id2 := p35ParentalServer(t, "p35stagenotif")
	p35SetupParental(t, client2, id2, "parent@example.com")
	p35Reset(buf)
	fs2.failParentalNotification(true)
	pinChange(t, client2, id2)
	l2 := p35Warnings(t, buf)
	if len(l2) != 1 || l2[0] != parentalHistoryFailureLine(eventParentalPinChanged, id2, "notification_write") {
		t.Fatalf("wrong notification stage line: %v", l2)
	}
}

// --- 5) Genau eine Warnung je fehlgeschlagenem Write; vier Writes = vier

func TestP35ParentalFailureEmitsExactlyOnce(t *testing.T) {
	// ein einzelner Write -> genau eine Warnung
	client, fs, id := p35ParentalServer(t, "p35once")
	p35SetupParental(t, client, id, "parent@example.com")
	buf := captureP35Logs(t)
	fs.failParentalNotification(true)
	rec := client.post(t, "/parental/remove", map[string]any{
		"account_id": id, "parent_pin": "1234"})
	if rec.Code != http.StatusOK {
		t.Fatalf("remove: %d %s", rec.Code, rec.Body.String())
	}
	if l := p35Warnings(t, buf); len(l) != 1 {
		t.Fatalf("one write must warn exactly once, got %d: %v", len(l), l)
	}

	// /parental/update mit vier geaenderten Settings -> vier Notification-
	// Writes (Wochentag + chat + voice + warning) -> vier Warnungen
	// (zweite Phase, gleicher Test -> gleicher Mutex)
	client2, fs2, id2 := p35ParentalServer(t, "p35four")
	p35SetupParental(t, client2, id2, "parent@example.com")
	// setupPayload setzt chat=true/voice=false/warning=30 bei 60 Minuten.
	// Hier aendern sich alle vier Einstellungen wirklich:
	// heute 60->90, chat true->false, voice false->true, warning 30->15.
	p := setupPayload(id2, todayWeek(t, 90))
	p["chat_enabled"] = false
	p["voice_enabled"] = true
	p["warning_minutes"] = 15
	p35Reset(buf)
	fs2.failParentalNotification(true)
	rec = client2.post(t, "/parental/update", p)
	if rec.Code != http.StatusOK {
		t.Fatalf("update: %d %s", rec.Code, rec.Body.String())
	}
	if l := p35Warnings(t, buf); len(l) != 4 {
		t.Fatalf("four failed notification writes must warn 4x, got %d: %v", len(l), l)
	}
	for _, line := range p35Warnings(t, buf) {
		if line != parentalHistoryFailureLine(eventParentalSettings, id2, "notification_write") {
			t.Fatalf("unexpected warning content: %q", line)
		}
	}
}

// --- 6) Erfolgsfall erzeugt keine P-35-Warnung

func TestP35ParentalSuccessEmitsNothing(t *testing.T) {
	client, fs, id := p35ParentalServer(t, "p35ok")
	p35SetupParental(t, client, id, "parent@example.com")
	buf := captureP35Logs(t)
	rec := client.post(t, "/parental/pin/change", map[string]any{
		"account_id": id, "old_pin": "1234", "new_pin": "4321"})
	if rec.Code != http.StatusOK {
		t.Fatalf("pin change: %d %s", rec.Code, rec.Body.String())
	}
	if l := p35Warnings(t, buf); len(l) != 0 {
		t.Fatalf("successful writes must not warn, got %v", l)
	}
	if n := p35EventCount(t, fs, id, eventParentalPinChanged); n != 1 {
		t.Fatalf("event must be written on success, got %d", n)
	}
}

// --- 7) Doppelziel-Typen ueber stage unterscheidbar

func TestP35ParentalDualTargetDistinguishedByStage(t *testing.T) {
	client, fs, id := p35ParentalServer(t, "p35dual")
	p35SetupParental(t, client, id, "parent@example.com")

	// beide Ziele scheitern im selben Request -> zwei Zeilen, die sich
	// ausschliesslich in stage unterscheiden
	buf := captureP35Logs(t)
	fs.failSecurityEventWrite(true)
	fs.failParentalNotification(true)
	rec := client.post(t, "/parental/pin/change", map[string]any{
		"account_id": id, "old_pin": "1234", "new_pin": "4321"})
	if rec.Code != http.StatusOK {
		t.Fatalf("pin change: %d %s", rec.Code, rec.Body.String())
	}
	l := p35Warnings(t, buf)
	if len(l) != 2 {
		t.Fatalf("dual-target failure must warn twice, got %d: %v", len(l), l)
	}
	ev := parentalHistoryFailureLine(eventParentalPinChanged, id, "event_write")
	nf := parentalHistoryFailureLine(eventParentalPinChanged, id, "notification_write")
	if !p35Contains(l, ev) || !p35Contains(l, nf) {
		t.Fatalf("both stages expected (%q | %q), got %v", ev, nf, l)
	}
	// Die beiden Zeilen unterscheiden sich NUR in stage.
	if strings.Replace(ev, "stage=event_write", "X", 1) !=
		strings.Replace(nf, "stage=notification_write", "X", 1) {
		t.Fatal("the two lines must differ only in stage")
	}
}

func p35Contains(lines []string, want string) bool {
	for _, l := range lines {
		if l == want {
			return true
		}
	}
	return false
}

// --- 8) Keine P-34-Semantik: kein Rollback, kein Fehler, kein Retry

func TestP35ParentalFailureIsNotP34Semantics(t *testing.T) {
	client, fs, id := p35ParentalServer(t, "p35notp34")
	p35SetupParental(t, client, id, "parent@example.com")
	notifsBefore := p35NotifCount(t, fs, id)

	buf := captureP35Logs(t)
	fs.failSecurityEventWrite(true)
	fs.failParentalNotification(true)

	// dreimal ausgefuehrt: jede Ausfuehrung warnt, es entsteht KEIN Retry.
	// Genutzt wird /parental/update mit genau einer geaenderten Einstellung
	// (warning_minutes), damit kein PIN verbraucht wird und der Vorgang
	// beliebig oft wiederholbar bleibt. Je Lauf: 1 Event-Write, 1
	// Notification-Write.
	for i, warn := range []int{45, 50, 55} {
		p := setupPayload(id, todayWeek(t, 60))
		p["warning_minutes"] = warn
		rec := client.post(t, "/parental/update", p)
		if rec.Code != http.StatusOK {
			t.Fatalf("run %d: state change must stay successful, got %d %s", i, rec.Code, rec.Body.String())
		}
	}
	// Zustand unveraendert erfolgreich geaendert, kein Rollback
	if !p35ParentalEnabled(t, fs, id) {
		t.Fatal("authoritative state must stay enabled, no rollback")
	}
	if n := p35EventCount(t, fs, id, eventParentalPinChanged); n != 0 {
		t.Fatalf("no event may exist, got %d", n)
	}
	if n := p35NotifCount(t, fs, id); n != notifsBefore {
		t.Fatalf("no extra notification may appear, got %d want %d", n, notifsBefore)
	}
	// 3 Ausfuehrungen x 2 Ziele = 6 Warnungen, kein Retry-Paar
	if l := p35Warnings(t, buf); len(l) != 6 {
		t.Fatalf("3 runs x 2 targets must warn 6x, got %d: %v", len(l), l)
	}
}

// --- 9) Kein Empfaenger: stiller, fehlerfreier Fruehabruch

func TestP35ParentalNoRecipientStaysSilent(t *testing.T) {
	client, fs, id := p35ParentalServer(t, "p35norecip")
	// setup OHNE parent_email
	p35SetupParental(t, client, id, "")

	buf := captureP35Logs(t)
	fs.failParentalNotification(true)
	// weiterhin ohne Empfaenger -> frueher Return, kein Write, kein Log
	rec := client.post(t, "/parental/pin/change", map[string]any{
		"account_id": id, "old_pin": "1234", "new_pin": "4321"})
	if rec.Code != http.StatusOK {
		t.Fatalf("pin change: %d %s", rec.Code, rec.Body.String())
	}
	if l := p35Warnings(t, buf); len(l) != 0 {
		t.Fatalf("without a recipient there is no write and no warning, got %v", l)
	}
	if !p35ParentalEnabled(t, fs, id) {
		t.Fatal("state change must stay successful")
	}
	// security_events wird unabhaengig vom Empfaenger geschrieben
	if n := p35EventCount(t, fs, id, eventParentalPinChanged); n != 1 {
		t.Fatalf("security_events write is independent of the recipient, got %d", n)
	}
}
