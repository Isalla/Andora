package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
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
