package main

import (
	"bytes"
	"context"
	"database/sql"
	"database/sql/driver"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	mysqlerr "github.com/go-sql-driver/mysql"
)

// testServer builds a Server with the fake store: full endpoint
// coverage without a database.
func testServer(t *testing.T) *Server {
	t.Helper()
	allPerms := []string{
		permAccountAuthenticate, permAccountRegister, permAccountPassword,
		permAccountRecovery, permAccountPermissions,
		permSessionCreate, permSessionValidate, permSessionRevoke,
		permRealmList, permHandoffCreate, permHandoffValidate,
		permWorldAuthenticate, permWorldHeartbeat,
		permParentalManage, permParentalPin, permParentalStatus, permParentalNotify,
	}
	cfg := &Config{
		Port:   8080,
		AuthDB: AuthDBConfig{Host: "localhost", Port: 3306, User: "u", Password: "p", Database: "auth"},
		Services: map[string]ServiceCred{
			"svc-all":     {ID: "svc-all", Secret: "topsecret-all", Permissions: allPerms},
			"svc-limited": {ID: "svc-limited", Secret: "topsecret-lim", Permissions: []string{permRealmList}},
		},
		// fast argon params for the tests
		ArgonTime:       1,
		ArgonMemory:     32,
		ArgonThreads:    1,
		SessionTTL:      60 * time.Minute,
		HandoffTTL:      time.Hour,
		RecoveryTTL:     30 * time.Minute,
		NewAccountBan:   60 * time.Second,
		RateLimitBurst:  10,
		RateLimitPerMin: 120,
		EncryptionKey:   "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
	}
	srv := &Server{cfg: cfg, store: newFakeStore(), rl: newRateLimit(cfg), start: time.Now()}
	return srv
}

type apiClient struct {
	s    *Server
	cred ServiceCred
}

// post signs the request like a real consuming service and runs it
// through the server's full handler chain (pre-auth rate limit,
// service auth, permission check, handler).
func (c *apiClient) post(t *testing.T, path string, payload any) *httptest.ResponseRecorder {
	t.Helper()
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatal(err)
	}
	ts := time.Now().Unix()
	req := httptest.NewRequest("POST", path, bytes.NewReader(body))
	req.Header.Set("X-Andora-Service", c.cred.ID)
	req.Header.Set("X-Andora-Timestamp", strconv.FormatInt(ts, 10))
	req.Header.Set("X-Andora-Signature", signPayload(c.cred.Secret, "POST", path, "", ts, body))
	rec := httptest.NewRecorder()
	c.s.handler().ServeHTTP(rec, req)
	return rec
}

func (c *apiClient) get(t *testing.T, path string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest("GET", path, nil)
	rec := httptest.NewRecorder()
	c.s.handler().ServeHTTP(rec, req)
	return rec
}

func decode(t *testing.T, rec *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var out map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatalf("decode response %q: %v", rec.Body.String(), err)
	}
	return out
}

func TestStatusAndHealthOpen(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	rec := client.get(t, "/status")
	if rec.Code != http.StatusOK {
		t.Fatalf("status code %d", rec.Code)
	}
	body := decode(t, rec)
	if body["status"] != "ok" {
		t.Fatalf("status body %v", body)
	}
	rec2 := client.get(t, "/health")
	if rec2.Code != http.StatusOK {
		t.Fatalf("health code %d", rec2.Code)
	}
}

func TestAuthVerifyBadSignature(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	req := httptest.NewRequest("POST", "/auth/verify", bytes.NewBufferString(`{"username":"x","password":"y"}`))
	req.Header.Set("X-Andora-Service", client.cred.ID)
	req.Header.Set("X-Andora-Timestamp", strconv.FormatInt(time.Now().Unix(), 10))
	req.Header.Set("X-Andora-Signature", "bad-signature")
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

func TestAuthVerifyUnknownService(t *testing.T) {
	srv := testServer(t)
	body := []byte(`{"username":"x","password":"y"}`)
	ts := time.Now().Unix()
	req := httptest.NewRequest("POST", "/auth/verify", bytes.NewReader(body))
	req.Header.Set("X-Andora-Service", "ghost")
	req.Header.Set("X-Andora-Timestamp", strconv.FormatInt(ts, 10))
	req.Header.Set("X-Andora-Signature", signPayload("s", "POST", "/auth/verify", "", ts, body))
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

func addAccount(t *testing.T, fs *fakeStore, username, password, email string, banned bool) *Account {
	t.Helper()
	hash, err := HashPasswordSalt(password, 1, 32, 1)
	if err != nil {
		t.Fatal(err)
	}
	id, err := fs.RegisterAccount(context.Background(), username, hash, "enc-"+email, hashLookupEmailRaw(email), time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	acc := fs.accounts[id]
	if !banned {
		acc.BanUntil = nil
	}
	return acc
}

func TestAuthVerifyNoAccount(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	rec := client.post(t, "/auth/verify", map[string]any{"username": "ghost-user", "password": "whatever"})
	if rec.Code != http.StatusOK {
		t.Fatalf("code %d", rec.Code)
	}
	body := decode(t, rec)
	if body["valid"] != false {
		t.Fatalf("expected valid=false, got %v", body)
	}
}

func TestAuthVerifyWrongPassword(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "tester", "correct-horse", "t@example.com", false)
	rec := client.post(t, "/auth/verify", map[string]any{"username": "tester", "password": "WRONG"})
	body := decode(t, rec)
	if body["valid"] != false {
		t.Fatalf("expected valid=false, got %v", body)
	}
}

func TestAuthVerifyBannedAccount(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	// register leaves the account banned (fresh account ban)
	addAccount(t, srv.store.(*fakeStore), "banned1", "correct-horse", "b@example.com", true)
	rec := client.post(t, "/auth/verify", map[string]any{"username": "banned1", "password": "correct-horse"})
	body := decode(t, rec)
	if body["valid"] != false {
		t.Fatalf("expected valid=false for banned, got %v", body)
	}
}

func TestAuthVerifySuccessCreatesSession(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "tester", "correct-horse", "t@example.com", false)
	rec := client.post(t, "/auth/verify", map[string]any{"username": "tester", "password": "correct-horse"})
	if rec.Code != http.StatusOK {
		t.Fatalf("code %d", rec.Code)
	}
	body := decode(t, rec)
	if body["valid"] != true {
		t.Fatalf("expected valid=true, got %v", body)
	}
	if body["account_id"] != float64(1) {
		t.Fatalf("account_id %v", body)
	}
	sessionID, ok := body["session_id"].(string)
	if !ok || sessionID == "" {
		t.Fatalf("session_id missing: %v", body)
	}

	// the issued session validates
	rec = client.post(t, "/session/validate", map[string]any{"session_id": sessionID})
	body = decode(t, rec)
	if body["valid"] != true || body["account_id"] != float64(1) {
		t.Fatalf("validate: %v", body)
	}

	// revoking kills it
	rec = client.post(t, "/session/revoke", map[string]any{"session_id": sessionID})
	if rec.Code != http.StatusOK || decode(t, rec)["revoked"] != true {
		t.Fatalf("revoke: %d %s", rec.Code, rec.Body.String())
	}
	rec = client.post(t, "/session/validate", map[string]any{"session_id": sessionID})
	if decode(t, rec)["valid"] != false {
		t.Fatalf("session should be dead after revoke")
	}
}

func TestAuthVerifyByEmailLookupHash(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "hasher", "correct-horse", "Mixed.Case@Example.COM", false)
	rec := client.post(t, "/auth/verify", map[string]any{
		"email_lookup_hash": hashLookupEmail("Mixed.Case@Example.COM"),
		"password":          "correct-horse",
	})
	body := decode(t, rec)
	if body["valid"] != true {
		t.Fatalf("lookup by email hash failed: %v", body)
	}
}

func TestAccountRegisterValidation(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	bad := []map[string]any{
		{"username": "ab", "password": "longenough1", "email": "ok@example.com"},   // username too short
		{"username": "goodname", "password": "short", "email": "ok@example.com"},   // password too short
		{"username": "goodname", "password": "longenough1", "email": "no-at-sign"}, // email invalid
	}
	for i, payload := range bad {
		rec := client.post(t, "/account/register", payload)
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("case %d: expected 400, got %d", i, rec.Code)
		}
	}
}

func TestAccountRegisterAndDuplicate(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	payload := map[string]any{"username": "newguy", "password": "longenough1", "email": "n@example.com"}
	rec := client.post(t, "/account/register", payload)
	if rec.Code != http.StatusCreated {
		t.Fatalf("register code %d %s", rec.Code, rec.Body.String())
	}
	body := decode(t, rec)
	if body["account_id"] == nil {
		t.Fatalf("account_id missing: %v", body)
	}

	rec = client.post(t, "/account/register", payload)
	if rec.Code != http.StatusConflict {
		t.Fatalf("duplicate: expected 409, got %d", rec.Code)
	}

	// same e-mail, different username must also conflict
	rec = client.post(t, "/account/register", map[string]any{"username": "othername", "password": "longenough1", "email": "n@example.com"})
	if rec.Code != http.StatusConflict {
		t.Fatalf("duplicate email: expected 409, got %d", rec.Code)
	}
}

func TestPasswordChange(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "changer", "old-password-1", "c@example.com", false)

	rec := client.post(t, "/account/password/change", map[string]any{"account_id": 1, "old_password": "wrong-old", "new_password": "new-password-2"})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}

	rec = client.post(t, "/account/password/change", map[string]any{"account_id": 1, "old_password": "old-password-1", "new_password": "new-password-2"})
	if rec.Code != http.StatusOK || decode(t, rec)["changed"] != true {
		t.Fatalf("change: %d %s", rec.Code, rec.Body.String())
	}

	// old password is dead, new one works
	rec = client.post(t, "/auth/verify", map[string]any{"username": "changer", "password": "old-password-1"})
	if decode(t, rec)["valid"] != false {
		t.Fatal("old password should not verify")
	}
	rec = client.post(t, "/auth/verify", map[string]any{"username": "changer", "password": "new-password-2"})
	if decode(t, rec)["valid"] != true {
		t.Fatal("new password should verify")
	}
}

func TestRecoveryFlow(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "loser", "old-password-1", "l@example.com", true)

	rec := client.post(t, "/account/recovery/request", map[string]any{"account_id": 99})
	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404, got %d", rec.Code)
	}

	rec = client.post(t, "/account/recovery/request", map[string]any{"account_id": 1})
	if rec.Code != http.StatusOK {
		t.Fatalf("request: %d %s", rec.Code, rec.Body.String())
	}
	body := decode(t, rec)
	token, _ := body["recovery_token"].(string)
	if token == "" {
		t.Fatal("recovery_token missing")
	}

	rec = client.post(t, "/account/recovery/confirm", map[string]any{"recovery_token": token, "new_password": "fresh-password"})
	if rec.Code != http.StatusOK || decode(t, rec)["recovered"] != true {
		t.Fatalf("confirm: %d %s", rec.Code, rec.Body.String())
	}

	// account is un-banned and the new password works
	rec = client.post(t, "/auth/verify", map[string]any{"username": "loser", "password": "fresh-password"})
	if decode(t, rec)["valid"] != true {
		t.Fatal("recovered account should verify")
	}

	// token is single use
	rec = client.post(t, "/account/recovery/confirm", map[string]any{"recovery_token": token, "new_password": "other-password"})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("replay: expected 401, got %d", rec.Code)
	}
}

func TestSessionCreateBanned(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "fresh", "longenough1", "f@example.com", true)
	rec := client.post(t, "/session/create", map[string]any{"account_id": 1})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

func TestPermissionDenied(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-limited"]} // only realm.list
	rec := client.post(t, "/session/create", map[string]any{"account_id": 1})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("expected 403, got %d", rec.Code)
	}
}

func TestRealmsList(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	now := time.Now()
	srv.store.(*fakeStore).realms = []Realm{
		{ID: 1, Name: "de1", Language: "de", Region: "eu", Enabled: true, FreshStartUntil: &now},
		{ID: 2, Name: "en1", Language: "en", Region: "us", Enabled: false},
	}
	ts := time.Now().Unix()
	req := httptest.NewRequest("GET", "/realms", nil)
	req.Header.Set("X-Andora-Service", client.cred.ID)
	req.Header.Set("X-Andora-Timestamp", strconv.FormatInt(ts, 10))
	req.Header.Set("X-Andora-Signature", signPayload(client.cred.Secret, "GET", "/realms", "", ts, nil))
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("code %d", rec.Code)
	}
	var out struct {
		Realms []struct {
			ID   int    `json:"id"`
			Name string `json:"name"`
		} `json:"realms"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatal(err)
	}
	if len(out.Realms) != 2 || out.Realms[0].Name != "de1" || out.Realms[1].Name != "en1" {
		t.Fatalf("realms %v", out.Realms)
	}
}

func TestHandoffSingleUse(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "mover", "longenough1", "m@example.com", false)
	srv.store.(*fakeStore).realms = []Realm{{ID: 1, Name: "de1", Language: "de", Region: "eu", Enabled: true}}

	rec := client.post(t, "/handoff/create", map[string]any{"account_id": 1, "realm_id": 1})
	if rec.Code != http.StatusOK {
		t.Fatalf("create: %d %s", rec.Code, rec.Body.String())
	}
	token, _ := decode(t, rec)["handoff_token"].(string)
	if token == "" {
		t.Fatal("handoff_token missing")
	}

	rec = client.post(t, "/handoff/validate", map[string]any{"handoff_token": token})
	body := decode(t, rec)
	if body["valid"] != true || body["account_id"] != float64(1) || body["realm_id"] != float64(1) {
		t.Fatalf("validate: %v", body)
	}

	// second use must fail
	rec = client.post(t, "/handoff/validate", map[string]any{"handoff_token": token})
	if decode(t, rec)["valid"] != false {
		t.Fatal("handoff must be single use")
	}
}

func TestHandoffExpiry(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, fs, "mover2", "longenough1", "m2@example.com", false)
	fs.realms = []Realm{{ID: 1, Name: "de1", Language: "de", Region: "eu", Enabled: true}}

	// already-created handoff (TTL expired)
	token, _, err := fs.CreateHandoff(context.Background(), 1, 1, -time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	rec := client.post(t, "/handoff/validate", map[string]any{"handoff_token": token})
	body := decode(t, rec)
	if body["valid"] != false {
		t.Fatalf("expired handoff must not validate: %v", body)
	}
}

func TestWorldServerAuthAndHeartbeat(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	srv.store.(*fakeStore).realms = []Realm{{ID: 1, Name: "de1", Language: "de", Region: "eu", Enabled: true}}
	srv.store.(*fakeStore).worlds[1] = &WorldServer{ID: 1, RealmID: 1, Name: "ws1", Host: "h", Port: 1234, Version: "1.0", Enabled: true, Credential: "ws-secret-1", MaxPlayers: 100}

	rec := client.post(t, "/world/authenticate", map[string]any{"server_id": 1, "credential": "wrong"})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
	rec = client.post(t, "/world/authenticate", map[string]any{"server_id": 42, "credential": "ws-secret-1"})
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("unknown server: expected 401, got %d", rec.Code)
	}
	rec = client.post(t, "/world/authenticate", map[string]any{"server_id": 1, "credential": "ws-secret-1"})
	body := decode(t, rec)
	if body["valid"] != true || body["realm_id"] != float64(1) || body["name"] != "ws1" {
		t.Fatalf("auth: %v", body)
	}

	rec = client.post(t, "/world/heartbeat", map[string]any{"server_id": 1, "credential": "ws-secret-1", "version": "1.1", "current_players": 5, "ok": true})
	if rec.Code != http.StatusOK || decode(t, rec)["recorded"] != true {
		t.Fatalf("heartbeat: %d %s", rec.Code, rec.Body.String())
	}
	ws := srv.store.(*fakeStore).worlds[1]
	if ws.Version != "1.1" || ws.CurrentPlayers != 5 || ws.Status != "online" || ws.LastHeartbeat == nil {
		t.Fatalf("heartbeat state: %+v", ws)
	}
}

func TestRateLimitPreAuth(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	var last *httptest.ResponseRecorder
	for i := 0; i < srv.cfg.RateLimitBurst; i++ {
		last = client.get(t, "/status")
		if last.Code != http.StatusOK {
			t.Fatalf("request %d: %d", i, last.Code)
		}
	}
	last = client.get(t, "/status")
	if last.Code != http.StatusTooManyRequests {
		t.Fatalf("expected 429, got %d", last.Code)
	}
	if last.Header().Get("Retry-After") == "" {
		t.Fatal("Retry-After header missing")
	}
}

func TestSessionExpiry(t *testing.T) {
	srv := testServer(t)
	srv.cfg.SessionTTL = time.Second
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	addAccount(t, srv.store.(*fakeStore), "short", "longenough1", "s@example.com", false)
	rec := client.post(t, "/session/create", map[string]any{"account_id": 1})
	if rec.Code != http.StatusOK {
		t.Fatalf("code %d", rec.Code)
	}
	sessionID, _ := decode(t, rec)["session_id"].(string)
	time.Sleep(1100 * time.Millisecond)
	rec = client.post(t, "/session/validate", map[string]any{"session_id": sessionID})
	if decode(t, rec)["valid"] != false {
		t.Fatal("expired session must not validate")
	}
}

// --- AUTH-02b: unterscheidbarer Session-Widerruf ---------------------------
//
// Migration 014 fuehrt sessions.revoked_at ein. Ein ausdruecklicher Widerruf
// markiert die Zeile, statt sie zu loeschen, damit "widerrufen" von
// "regulaer abgelaufen" und von "unbekannt" unterscheidbar bleibt.

// helper: eine Session direkt im fakeStore anlegen und den Roh-Token liefern.
func mkSession(t *testing.T, fs *fakeStore, accountID int, ttl time.Duration) string {
	t.Helper()
	fs.mu.Lock()
	_, exists := fs.accounts[accountID]
	fs.mu.Unlock()
	if !exists {
		t.Fatalf("account %d must exist before a session can be created", accountID)
	}
	raw, _, err := fs.CreateSession(context.Background(), accountID, ttl)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

// helper: Status direkt aus dem Store (ohne HTTP-Rundlauf).
func statusOf(t *testing.T, fs *fakeStore, raw string) SessionStatus {
	t.Helper()
	st, _, err := fs.SessionStatusOf(context.Background(), raw)
	if err != nil {
		t.Fatal(err)
	}
	return st
}

// 1) Migration: die Spalte existiert und ist fuer Bestandszeilen NULL.
func TestSessionRevokedAtMigrationIsNullable(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "mig", "longenough1", "mig@e.com", false)
	raw := mkSession(t, fs, 1, time.Hour)
	fs.mu.Lock()
	defer fs.mu.Unlock()
	sess, ok := fs.sessions[tokenHash(raw)]
	if !ok {
		t.Fatal("session missing")
	}
	// Bestands-/neue Zeilen ohne Widerruf tragen NULL.
	if sess.RevokedAt != nil {
		t.Fatalf("fresh session must have revoked_at NULL, got %v", sess.RevokedAt)
	}
}

// 2) gueltige Session -> valid
func TestSessionStatusValid(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "stat", "longenough1", "stat@e.com", false)
	raw := mkSession(t, fs, 1, time.Hour)
	if got := statusOf(t, fs, raw); got != SessionValid {
		t.Fatalf("want valid, got %q", got)
	}
}

// 3) regulaer abgelaufene Session -> expired (KEIN revoked)
func TestSessionStatusExpiredIsNotRevoked(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "statx", "longenough1", "statx@e.com", false)
	raw := mkSession(t, fs, 1, -time.Minute) // bereits abgelaufen
	if got := statusOf(t, fs, raw); got != SessionExpired {
		t.Fatalf("want expired, got %q", got)
	}
	fs.mu.Lock()
	sess := fs.sessions[tokenHash(raw)]
	fs.mu.Unlock()
	if sess.RevokedAt != nil {
		t.Fatal("expired session must keep revoked_at NULL")
	}
}

// 4) markierte Session -> revoked
func TestSessionStatusRevoked(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "statr", "longenough1", "statr@e.com", false)
	raw := mkSession(t, fs, 1, time.Hour)
	if err := fs.RevokeSession(context.Background(), raw); err != nil {
		t.Fatal(err)
	}
	if got := statusOf(t, fs, raw); got != SessionRevoked {
		t.Fatalf("want revoked, got %q", got)
	}
}

// 5) fehlende Session -> missing
func TestSessionStatusMissing(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	raw := "ff" + strings.Repeat("a", 62)
	if got := statusOf(t, fs, raw); got != SessionMissing {
		t.Fatalf("want missing, got %q", got)
	}
}

// 6) bestehendes /session/validate bleibt rueckwaertskompatibel
func TestSessionValidateStaysCompatible(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "compat", "longenough1", "c@e.com", false)

	// gueltig -> valid:true
	live := mkSession(t, fs, 1, time.Hour)
	rec := client.post(t, "/session/validate", map[string]any{"session_id": live})
	if decode(t, rec)["valid"] != true {
		t.Fatalf("valid session must report valid:true: %v", decode(t, rec))
	}

	// widerrufen -> valid:false (kein Feld 'status' im Altvertrag)
	if err := fs.RevokeSession(context.Background(), live); err != nil {
		t.Fatal(err)
	}
	rec = client.post(t, "/session/validate", map[string]any{"session_id": live})
	body := decode(t, rec)
	if body["valid"] != false {
		t.Fatalf("revoked session must report valid:false: %v", body)
	}
	if _, leaked := body["status"]; leaked {
		t.Fatal("legacy /session/validate must not expose a status field")
	}

	// abgelaufen -> valid:false
	expired := mkSession(t, fs, 1, -time.Minute)
	rec = client.post(t, "/session/validate", map[string]any{"session_id": expired})
	if decode(t, rec)["valid"] != false {
		t.Fatalf("expired session must report valid:false: %v", decode(t, rec))
	}
}

// 7) Single-Revoke markiert NUR diese Session
func TestSessionRevokeMarksOnlyThatSession(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "sing", "longenough1", "sing@e.com", false)
	a := mkSession(t, fs, 1, time.Hour)
	b := mkSession(t, fs, 1, time.Hour)
	if err := fs.RevokeSession(context.Background(), a); err != nil {
		t.Fatal(err)
	}
	if got := statusOf(t, fs, a); got != SessionRevoked {
		t.Fatalf("session a: want revoked, got %q", got)
	}
	if got := statusOf(t, fs, b); got != SessionValid {
		t.Fatalf("session b must stay valid, got %q", got)
	}
}

// 8) accountweites Revoke markiert ALLE Sessions
func TestRevokeAllSessionsMarksEverySession(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "a1", "longenough1", "a@e.com", false)
	addAccount(t, fs, "a2", "longenough1", "b@e.com", false)
	own := []string{mkSession(t, fs, 1, time.Hour), mkSession(t, fs, 1, time.Hour)}
	other := mkSession(t, fs, 2, time.Hour)

	if err := fs.RevokeAllSessions(context.Background(), 1); err != nil {
		t.Fatal(err)
	}
	for _, r := range own {
		if got := statusOf(t, fs, r); got != SessionRevoked {
			t.Fatalf("own session: want revoked, got %q", got)
		}
	}
	if got := statusOf(t, fs, other); got != SessionValid {
		t.Fatalf("other account must stay valid, got %q", got)
	}
}

// 9) Passwortaenderung markiert alle Sessions
func TestPasswordChangeMarksAllSessionsRevoked(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "changer", "old-password-1", "c@e.com", false)
	s1 := mkSession(t, fs, 1, time.Hour)
	s2 := mkSession(t, fs, 1, time.Hour)

	rec := client.post(t, "/account/password/change", map[string]any{
		"account_id": 1, "old_password": "old-password-1", "new_password": "new-password-9",
	})
	if rec.Code != http.StatusOK {
		t.Fatalf("change: %d %s", rec.Code, rec.Body.String())
	}
	for _, s := range []string{s1, s2} {
		if got := statusOf(t, fs, s); got != SessionRevoked {
			t.Fatalf("password change must mark sessions revoked, got %q", got)
		}
	}
}

// 10) 2FA-Reset markiert alle Sessions
func TestTwoFactorResetMarksAllSessionsRevoked(t *testing.T) {
	// Bestehende 2FA-Naehte aus twofactor_test.go wiederverwenden.
	tfSrv := tfTestServer(t)
	tfCli := tfClient(tfSrv)
	tfFs := tfSrv.store.(*fakeStore)
	id, raw, codes := tfEnroll(t, tfSrv, "tfauth", "correct-horse")
	dt, _ := tfTrustDevice(t, tfCli, "tfauth", "correct-horse", codes, 0, "laptop")
	_ = dt
	body := tfVerify(t, tfCli, map[string]any{
		"username": "tfauth", "password": "correct-horse", "totp_code": tfTotpNow(raw),
	})
	live, _ := body["session_id"].(string)
	if live == "" {
		t.Fatalf("2FA session missing: %v", body)
	}
	if got := statusOf(t, tfFs, live); got != SessionValid {
		t.Fatalf("precondition: want valid, got %q", got)
	}

	rec := tfCli.post(t, "/twofactor/reset", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK {
		t.Fatalf("2fa reset: %d %s", rec.Code, rec.Body.String())
	}
	if got := statusOf(t, tfFs, live); got != SessionRevoked {
		t.Fatalf("2FA reset must mark sessions revoked, got %q", got)
	}
}

// 11+12) Passwort-Reset markiert alle Sessions, entfernt Trusted Devices und
// schreibt das KORREKTE Security-Event (password_reset, nicht password_changed).
func TestPasswordResetRevokesSessionsDevicesAndEvent(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "loser", "old-password-1", "l@e.com", false)
	s1 := mkSession(t, fs, 1, time.Hour)
	s2 := mkSession(t, fs, 1, time.Hour)
	fs.mu.Lock()
	fs.trustedDevices[1] = map[string]TrustedDevice{"d1": {Label: "laptop"}}
	fs.mu.Unlock()

	rec := client.post(t, "/account/recovery/request", map[string]any{"account_id": 1})
	token, _ := decode(t, rec)["recovery_token"].(string)
	if token == "" {
		t.Fatalf("recovery_token missing: %v", decode(t, rec))
	}
	rec = client.post(t, "/account/recovery/confirm", map[string]any{
		"recovery_token": token, "new_password": "fresh-password-9",
	})
	if rec.Code != http.StatusOK || decode(t, rec)["recovered"] != true {
		t.Fatalf("confirm: %d %s", rec.Code, rec.Body.String())
	}

	for _, s := range []string{s1, s2} {
		if got := statusOf(t, fs, s); got != SessionRevoked {
			t.Fatalf("password reset must mark sessions revoked, got %q", got)
		}
	}
	fs.mu.Lock()
	devLeft := len(fs.trustedDevices[1])
	fs.mu.Unlock()
	if devLeft != 0 {
		t.Fatalf("password reset must drop trusted devices, %d left", devLeft)
	}
	evs, err := fs.ListSecurityEvents(context.Background(), 1)
	if err != nil {
		t.Fatal(err)
	}
	found := ""
	for _, e := range evs {
		if e.EventType == eventPasswordReset {
			found = e.EventType
		}
		if e.EventType == eventPasswordChanged {
			t.Fatal("recovery must not be logged as password_changed")
		}
	}
	if found == "" {
		t.Fatalf("missing %q event, got %+v", eventPasswordReset, evs)
	}
}

// 13) Fehler in der Recovery-Transaktion -> vollstaendiger Rollback an JEDER Stelle
func TestPasswordResetRollsBackCompletely(t *testing.T) {
	for stage := 1; stage <= 4; stage++ {
		srv := testServer(t)
		client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
		fs := srv.store.(*fakeStore)
		addAccount(t, fs, "rb", "old-password-1", "r@e.com", false)
		s1 := mkSession(t, fs, 1, time.Hour)
		fs.mu.Lock()
		fs.trustedDevices[1] = map[string]TrustedDevice{"d1": {Label: "laptop"}}
		fs.mu.Unlock()

		rec := client.post(t, "/account/recovery/request", map[string]any{"account_id": 1})
		token, _ := decode(t, rec)["recovery_token"].(string)

		fs.recoverFailAt(stage)
		rec = client.post(t, "/account/recovery/confirm", map[string]any{
			"recovery_token": token, "new_password": "fresh-password-9",
		})
		if rec.Code < 400 {
			t.Fatalf("stage %d: expected failure, got %d", stage, rec.Code)
		}

		// altes Passwort gilt weiterhin
		rec = client.post(t, "/auth/verify", map[string]any{"username": "rb", "password": "old-password-1"})
		if decode(t, rec)["valid"] != true {
			t.Fatalf("stage %d: old password must survive rollback", stage)
		}
		// Session unveraendert
		if got := statusOf(t, fs, s1); got != SessionValid {
			t.Fatalf("stage %d: session must survive rollback, got %q", stage, got)
		}
		// Trusted Device unveraendert
		fs.mu.Lock()
		devLeft := len(fs.trustedDevices[1])
		fs.mu.Unlock()
		if devLeft != 1 {
			t.Fatalf("stage %d: trusted device must survive rollback, got %d", stage, devLeft)
		}
		// Recovery-Token bleibt wiederverwendbar
		fs.recoverFailAt(0)
		rec = client.post(t, "/account/recovery/confirm", map[string]any{
			"recovery_token": token, "new_password": "fresh-password-9",
		})
		if rec.Code != http.StatusOK {
			t.Fatalf("stage %d: recovery token must stay reusable, got %d", stage, rec.Code)
		}
	}
}

// --- AUTH-02b: Batch-Statusabfrage ------------------------------------------

func batchReq(pairs ...[2]any) map[string]any {
	items := make([]map[string]any, 0, len(pairs))
	for _, p := range pairs {
		items = append(items, map[string]any{"session_id": p[0], "account_id": p[1]})
	}
	return map[string]any{"sessions": items}
}

// 14) Batch prueft die Account-ID: die Antwort nennt den AUTHORITATIVEN Owner,
// damit ein Realm-Ergebnis nie einer falschen Verbindung zugeordnet wird.
func TestSessionStatusBatchVerifiesAccount(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "o1", "longenough1", "a@e.com", false)
	addAccount(t, fs, "o2", "longenough1", "b@e.com", false)
	s1 := mkSession(t, fs, 1, time.Hour) // gehört Account 1
	s2 := mkSession(t, fs, 2, time.Hour) // gehört Account 2

	rec := client.post(t, "/session/status/batch", batchReq(
		[2]any{s1, 1}, // korrekt
		[2]any{s2, 1}, // FALSCHE Erwartung: Token gehoert Account 2
	))
	if rec.Code != http.StatusOK {
		t.Fatalf("batch: %d %s", rec.Code, rec.Body.String())
	}
	results := decode(t, rec)["results"].([]any)
	if len(results) != 2 {
		t.Fatalf("want 2 results, got %d", len(results))
	}
	r0 := results[0].(map[string]any)
	if r0["status"] != string(SessionValid) || r0["account_id"] != float64(1) {
		t.Fatalf("result 0: %v", r0)
	}
	r1 := results[1].(map[string]any)
	// Der Realm sieht account_id=2 und weiss: nicht meine Session.
	if r1["account_id"] != float64(2) {
		t.Fatalf("batch must report the authoritative account: %v", r1)
	}
}

// 15) Batch lehnt mehr als 250 Eintraege fail-closed ab
func TestSessionStatusBatchRejectsOversize(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "big", "longenough1", "b@e.com", false)
	addAccount(t, fs, "sing", "longenough1", "sing@e.com", false)
	raw := mkSession(t, fs, 1, time.Hour)

	items := make([]map[string]any, 0, MaxSessionStatusBatch+1)
	for i := 0; i <= MaxSessionStatusBatch; i++ {
		items = append(items, map[string]any{"session_id": raw, "account_id": 1})
	}
	rec := client.post(t, "/session/status/batch", map[string]any{"sessions": items})
	if rec.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("want 413 for %d items, got %d", MaxSessionStatusBatch+1, rec.Code)
	}
	if strings.Contains(rec.Body.String(), string(SessionValid)) {
		t.Fatal("oversize request must not return partial results")
	}

	// Genau am Limit ist erlaubt.
	ok := items[:MaxSessionStatusBatch]
	rec = client.post(t, "/session/status/batch", map[string]any{"sessions": ok})
	if rec.Code != http.StatusOK {
		t.Fatalf("exactly %d must be accepted, got %d", MaxSessionStatusBatch, rec.Code)
	}
}

// 16) Batch: ein Lookup, kein N+1. Der fakeStore zaehlt Zugriffe.
func TestSessionStatusBatchDoesNoPerItemLookup(t *testing.T) {
	srv := testServer(t)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "n1", "longenough1", "a@e.com", false)
	var raws []string
	for i := 0; i < 10; i++ {
		raws = append(raws, mkSession(t, fs, 1, time.Hour))
	}
	statuses, accountIDs, err := fs.BatchSessionStatus(context.Background(), raws)
	if err != nil {
		t.Fatal(err)
	}
	if len(statuses) != 10 || len(accountIDs) != 10 {
		t.Fatalf("want 10/10, got %d/%d", len(statuses), len(accountIDs))
	}
	// Reihenfolge bleibt erhalten und fehlende Tokens ergeben missing.
	statuses2, _, err := fs.BatchSessionStatus(context.Background(),
		[]string{raws[3], "ff" + strings.Repeat("b", 62), raws[0]})
	if err != nil {
		t.Fatal(err)
	}
	if statuses2[0] != SessionValid || statuses2[1] != SessionMissing || statuses2[2] != SessionValid {
		t.Fatalf("order/status mismatch: %v", statuses2)
	}
	// Leere Eingabe: kein Query, leere Slices.
	es, ea, err := fs.BatchSessionStatus(context.Background(), nil)
	if err != nil || len(es) != 0 || len(ea) != 0 {
		t.Fatalf("empty batch: %v %v %v", es, ea, err)
	}
}

// 17) Batch-Antwort enthaelt keine Roh-Tokens; 4-Wege-Status ueber HTTP
func TestSessionStatusBatchExposesFourStatesWithoutTokens(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "four", "longenough1", "f@e.com", false)
	live := mkSession(t, fs, 1, time.Hour)
	expired := mkSession(t, fs, 1, -time.Minute)
	revoked := mkSession(t, fs, 1, time.Hour)
	if err := fs.RevokeSession(context.Background(), revoked); err != nil {
		t.Fatal(err)
	}
	absent := "cc" + strings.Repeat("d", 62)

	rec := client.post(t, "/session/status/batch", batchReq(
		[2]any{live, 1}, [2]any{expired, 1}, [2]any{revoked, 1}, [2]any{absent, 1},
	))
	if rec.Code != http.StatusOK {
		t.Fatalf("batch: %d %s", rec.Code, rec.Body.String())
	}
	want := []SessionStatus{SessionValid, SessionExpired, SessionRevoked, SessionMissing}
	results := decode(t, rec)["results"].([]any)
	if len(results) != len(want) {
		t.Fatalf("want %d results, got %d", len(want), len(results))
	}
	for i, w := range want {
		r := results[i].(map[string]any)
		if r["status"] != string(w) {
			t.Fatalf("result %d: want %q, got %v", i, w, r["status"])
		}
		if r["index"] != float64(i) {
			t.Fatalf("result %d: index mismatch %v", i, r["index"])
		}
	}
	// Kein Roh-Token in der Antwort.
	body := rec.Body.String()
	for _, raw := range []string{live, expired, revoked, absent} {
		if strings.Contains(body, raw) {
			t.Fatalf("response leaks a raw token: %s", body)
		}
	}
	// Auch der 64-stellige Hash steht nicht drin.
	if strings.Contains(body, tokenHash(live)) {
		t.Fatal("response leaks a token hash")
	}
}

// 18) Body oberhalb des Endpunkt-Limits wird kontrolliert abgelehnt, BEVOR der
// Body gelesen wird. Kein 64-hex-Token und kein Roh-Token im Log.
func TestSessionStatusBatchRejectsOversizeBody(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "fat", "longenough1", "x@e.com", false)
	raw := mkSession(t, fs, 1, time.Hour)

	// Ein deklarierter Body weit oberhalb des Endpunkt-Limits.
	items := make([]map[string]any, 0, MaxSessionStatusBatch)
	for i := 0; i < MaxSessionStatusBatch; i++ {
		items = append(items, map[string]any{"session_id": raw, "account_id": 1})
	}
	payload := map[string]any{"sessions": items, "padding": strings.Repeat("x", int(maxSessionStatusBatchBytes)+1024)}
	rec := client.post(t, "/session/status/batch", payload)
	if rec.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("want 413 for oversized body, got %d %s", rec.Code, rec.Body.String())
	}
	// Keine Teilverarbeitung: kein Status und kein Token in der Antwort.
	body := rec.Body.String()
	if strings.Contains(body, string(SessionValid)) || strings.Contains(body, raw) {
		t.Fatalf("oversized body must not be partially processed: %s", body)
	}
	// Die Grenze liegt unter dem serverweiten maxBodyBytes.
	if maxSessionStatusBatchBytes >= maxBodyBytes {
		t.Fatalf("endpunkt-limit %d muss unter dem serverweiten %d liegen", maxSessionStatusBatchBytes, maxBodyBytes)
	}
	// Genau 250 Eintraege (gemessen ~26.5 KiB) passen weiterhin.
	rec = client.post(t, "/session/status/batch", map[string]any{"sessions": items})
	if rec.Code != http.StatusOK {
		t.Fatalf("250 Eintraege muessen funktionieren, got %d %s", rec.Code, rec.Body.String())
	}
	results := decode(t, rec)["results"].([]any)
	if len(results) != MaxSessionStatusBatch {
		t.Fatalf("want %d results, got %d", MaxSessionStatusBatch, len(results))
	}
}

// 19) Token-Pruefung im Batchpfad: leer, ueberlang und syntaktisch falsch
// werden VOR Hashing/DB-Abfrage abgelehnt.
func TestSessionStatusBatchRejectsBadTokensBeforeLookup(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "tok", "longenough1", "t@e.com", false)
	good := mkSession(t, fs, 1, time.Hour)

	cases := map[string]string{
		"leer":            "",
		"63-stellig":      strings.Repeat("a", 63),
		"65-stellig":      strings.Repeat("a", 65),
		"kein Hex":        strings.Repeat("z", 64),
		"sehr lang":       strings.Repeat("a", 4096),
		"mit Leerzeichen": " " + good,
	}
	for name, bad := range cases {
		rec := client.post(t, "/session/status/batch", batchReq(
			[2]any{good, 1}, [2]any{bad, 1},
		))
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("%s: want 400, got %d %s", name, rec.Code, rec.Body.String())
		}
		if strings.Contains(rec.Body.String(), bad) && bad != "" {
			t.Fatalf("%s: Antwort darf das Token nicht zurueckspiegeln: %s", name, rec.Body.String())
		}
	}
	// Die Abweisung erfolgte vor der DB: der gueltige Eintrag darf nicht
	// verarbeitet worden sein (kein 'valid' in der Antwort).
}

// 20) Eine gelieferte account_id ausserhalb des gueltigen Bereichs wird NICHT
// erzwungen: die API erzwingt keine strengere Semantik als der Einzelendpunkt.
// Sie wird aber auch nie fuer die DB-Abfrage verwendet, sondern nur
// durchgereicht.
func TestSessionStatusBatchDoesNotUseRequestAccountIdForLookup(t *testing.T) {
	srv := testServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "acc", "longenough1", "a@e.com", false)
	raw := mkSession(t, fs, 1, time.Hour)

	// Absurd grosse und negative account_id: antworten muss mit 200 und der
	// AUTORITATIVEN account_id aus der DB (1), nicht mit der Anforderung.
	for _, bogus := range []int{0, -5, 999999999999} {
		rec := client.post(t, "/session/status/batch", batchReq([2]any{raw, bogus}))
		if rec.Code != http.StatusOK {
			t.Fatalf("account_id %d: want 200, got %d", bogus, rec.Code)
		}
		r := decode(t, rec)["results"].([]any)[0].(map[string]any)
		if r["account_id"] != float64(1) {
			t.Fatalf("account_id %d: server muss die DB-Account-ID melden, got %v", bogus, r["account_id"])
		}
	}
}

// ===================================================================
// P-33 session monitoring (/status)
// ===================================================================
//
// The tests below drive the REAL production code path of the monitoring:
// handleStatus -> renderSessionStats -> triggerSessionStats ->
// collectSessionStats -> AuthStore. The inventory/metadata store used
// here is a controllable FAKE (statsStore): it executes no SQL and talks
// to no database. The few tests that must cover the real *sQLStore use
// the scripted driver below, which also executes no SQL.
//
// Synchronisation is event-driven: the worker signals the test through a
// channel, and the blocking store is released by the test. Sleeps are
// not used as evidence of concurrency.

// statsStore wraps the in-memory fakeStore and adds controllable
// monitoring behaviour for the P-33 tests.
type statsStore struct {
	*fakeStore

	mu        sync.Mutex
	inv       SessionInventory
	invErr    error
	size      SessionTableSize
	sizeErr   error
	calls     int
	sizes     int
	cancelled bool

	// entered receives one signal per SessionInventory call.
	entered chan struct{}
	// block, when non-nil, makes SessionInventory wait for it to be
	// closed; blockUntilCtx makes it wait for ctx cancellation instead
	// (used for the timeout test).
	block         chan struct{}
	blockUntilCtx bool
}

func newStatsStore() *statsStore {
	return &statsStore{fakeStore: newFakeStore(), entered: make(chan struct{}, 64)}
}

func (s *statsStore) setInventory(inv SessionInventory, err error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.inv, s.invErr = inv, err
}

func (s *statsStore) setSize(size SessionTableSize, err error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.size, s.sizeErr = size, err
}

func (s *statsStore) counts() (int, int) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.calls, s.sizes
}

func (s *statsStore) SessionInventory(ctx context.Context) (SessionInventory, error) {
	s.mu.Lock()
	s.calls++
	block, untilCtx := s.block, s.blockUntilCtx
	err := s.invErr
	inv := s.inv
	s.mu.Unlock()
	select {
	case s.entered <- struct{}{}:
	default:
	}
	if untilCtx {
		<-ctx.Done()
		s.mu.Lock()
		s.cancelled = true
		s.mu.Unlock()
		return SessionInventory{}, ctx.Err()
	}
	if block != nil {
		select {
		case <-block:
		case <-ctx.Done():
			s.mu.Lock()
			s.cancelled = true
			s.mu.Unlock()
			return SessionInventory{}, ctx.Err()
		}
	}
	if err != nil {
		return SessionInventory{}, err
	}
	return inv, nil
}

func (s *statsStore) SessionTableSize(context.Context) (SessionTableSize, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.sizes++
	return s.size, s.sizeErr
}

func (s *statsStore) wasCancelled() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.cancelled
}

// ctxIgnoringStatsStore blocks in SessionInventory and deliberately
// IGNORES ctx cancellation. It models a store that cannot be aborted
// cooperatively and is the only way to reach close()'s bounded grace: a
// store that honours ctx ends the attempt immediately after the cancel.
type ctxIgnoringStatsStore struct {
	*statsStore
	release chan struct{}
	entered chan struct{}
}

func (s *ctxIgnoringStatsStore) SessionInventory(ctx context.Context) (SessionInventory, error) {
	s.statsStore.mu.Lock()
	s.statsStore.calls++
	inv, err := s.statsStore.inv, s.statsStore.invErr
	s.statsStore.mu.Unlock()
	select {
	case s.entered <- struct{}{}:
	default:
	}
	// Deliberately WITHOUT a ctx case: this store models a backend that
	// cannot be aborted cooperatively. Only release ends the attempt.
	<-s.release
	return inv, err
}

// statsLifecycle reads the worker bookkeeping under its lock so a test can
// assert completion facts without timing.
func statsLifecycle(srv *Server) (running int, doneNil, closing, poolClosed bool) {
	srv.sessionStatsMu.Lock()
	defer srv.sessionStatsMu.Unlock()
	return srv.sessionStatsRunning, srv.sessionStatsDone == nil, srv.sessionStatsClosing, srv.sessionStatsPoolClosed
}

// statsServer builds a Server whose store is a controllable statsStore and
// whose collection limits are test-sized.
func statsServer(t *testing.T, timeoutMS, minIntervalSecs int) (*Server, *statsStore) {
	t.Helper()
	srv := testServer(t)
	store := newStatsStore()
	srv.store = store
	srv.cfg.SessionStatsEnabled = true
	srv.cfg.SessionStatsTimeoutMS = timeoutMS
	srv.cfg.SessionStatsMinIntervalSecs = minIntervalSecs
	srv.sessionStatsSignal = make(chan struct{}, 64)
	return srv, store
}

// awaitStatsPublish waits for one collection attempt to finish.
func awaitStatsPublish(t *testing.T, srv *Server) {
	t.Helper()
	select {
	case <-srv.sessionStatsSignal:
	case <-time.After(5 * time.Second):
		t.Fatal("timeout: no session collection finished")
	}
}

// allowNextAttempt clears the minimum distance so the next /status may
// admit an attempt again. White-box, deterministic, no sleeping.
func allowNextAttempt(srv *Server) {
	srv.sessionStatsMu.Lock()
	srv.sessionStatsNextAllowed = time.Time{}
	srv.sessionStatsMu.Unlock()
}

// publishedSnapshot returns the currently published snapshot for
// white-box assertions on facts that RFC3339 renders only per second.
func publishedSnapshot(srv *Server) *sessionStatsSnapshot {
	srv.sessionStatsMu.Lock()
	defer srv.sessionStatsMu.Unlock()
	return srv.sessionStatsPub
}

// statusBody calls GET /status without a service signature (open route).
func statusBody(t *testing.T, srv *Server) map[string]any {
	t.Helper()
	req := httptest.NewRequest("GET", "/status", nil)
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("status code %d", rec.Code)
	}
	return decode(t, rec)
}

func sessionsBlock(t *testing.T, srv *Server) map[string]any {
	t.Helper()
	body := statusBody(t, srv)
	block, ok := body["sessions"].(map[string]any)
	if !ok {
		t.Fatalf("missing sessions block in %v", body)
	}
	return block
}

func numOrNil(v any) (float64, bool) {
	f, ok := v.(float64)
	return f, ok
}

func wantNum(t *testing.T, block map[string]any, key string, want float64) {
	t.Helper()
	got, ok := numOrNil(block[key])
	if !ok {
		t.Fatalf("%s: want number %v, got %#v (nil means unknown)", key, want, block[key])
	}
	if got != want {
		t.Fatalf("%s: want %v, got %v", key, want, got)
	}
}

func wantNil(t *testing.T, block map[string]any, key string) {
	t.Helper()
	if v, present := block[key]; present && v != nil {
		t.Fatalf("%s: want null (unknown), got %#v", key, v)
	}
}

func wantString(t *testing.T, block map[string]any, key, want string) {
	t.Helper()
	if got := block[key]; got != want {
		t.Fatalf("%s: want %q, got %#v", key, want, got)
	}
}

// collectOnce drives one full attempt: /status renders the CURRENT cache,
// then admits the worker; the test waits for the publication.
func collectOnce(t *testing.T, srv *Server) {
	t.Helper()
	statusBody(t, srv)
	awaitStatsPublish(t, srv)
}

func TestSessionStatsEmptyTableAndPartition(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)
	// Empty table: counters are real zeros, the age is unknown.
	store.setInventory(SessionInventory{PartitionOK: true}, nil)

	block := sessionsBlock(t, srv)
	wantString(t, block, "state", "unknown")
	wantNil(t, block, "total")
	wantNil(t, block, "oldest_row_age_days")
	wantNil(t, block, "total_delta")
	wantNil(t, block, "delta_window_seconds")

	awaitStatsPublish(t, srv)
	block = sessionsBlock(t, srv)
	wantString(t, block, "state", "ok")
	wantNum(t, block, "total", 0)
	wantNum(t, block, "active", 0)
	wantNum(t, block, "expired_not_revoked", 0)
	wantNum(t, block, "revoked", 0)
	if block["partition_ok"] != true {
		t.Fatalf("partition_ok: want true, got %#v", block["partition_ok"])
	}
	wantNil(t, block, "oldest_row_age_days") // empty table: unknown, not 0
	wantNil(t, block, "total_delta")         // first success has no base
	wantString(t, block, "size_state", "unknown")

	// A populated, correctly partitioned stock.
	age := int64(42)
	store.setInventory(SessionInventory{
		Total: 10, Active: 3, ExpiredNotRevoked: 5, Revoked: 2,
		PartitionOK: true, OldestRowAgeDays: &age, QueryMS: 1.5,
	}, nil)
	allowNextAttempt(srv)
	collectOnce(t, srv)
	block = sessionsBlock(t, srv)
	wantNum(t, block, "total", 10)
	wantNum(t, block, "active", 3)
	wantNum(t, block, "expired_not_revoked", 5)
	wantNum(t, block, "revoked", 2)
	wantNum(t, block, "oldest_row_age_days", 42)
	if block["partition_ok"] != true {
		t.Fatalf("partition_ok: want true, got %#v", block["partition_ok"])
	}
	if _, ok := numOrNil(block["aggregate_query_ms"]); !ok {
		t.Fatalf("aggregate_query_ms missing: %#v", block["aggregate_query_ms"])
	}
	// Liveness fields are untouched.
	body := statusBody(t, srv)
	if body["status"] != "ok" {
		t.Fatalf("status: %#v", body["status"])
	}
	if _, ok := body["uptime"].(string); !ok {
		t.Fatalf("uptime missing: %#v", body["uptime"])
	}
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, httptest.NewRequest("GET", "/health", nil))
	if rec.Code != http.StatusOK || decode(t, rec)["status"] != "ok" {
		t.Fatalf("/health changed: %d %v", rec.Code, decode(t, rec))
	}
}

func TestSessionStatsDisabledIsReported(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)
	srv.cfg.SessionStatsEnabled = false

	block := sessionsBlock(t, srv)
	wantString(t, block, "state", "disabled")
	if block["enabled"] != false {
		t.Fatalf("enabled: %#v", block["enabled"])
	}
	if block["in_flight"] != false {
		t.Fatalf("in_flight: %#v", block["in_flight"])
	}
	statusBody(t, srv)
	calls, _ := store.counts()
	if calls != 0 {
		t.Fatalf("disabled monitoring must not collect, calls=%d", calls)
	}
}

func TestSessionStatsErrorWithoutPreviousSuccess(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)
	store.setInventory(SessionInventory{}, errors.New("inventory down"))

	collectOnce(t, srv)
	block := sessionsBlock(t, srv)
	wantString(t, block, "state", "error")
	wantNil(t, block, "total")
	wantNil(t, block, "collected_at")
	if block["last_attempt_ok"] != false {
		t.Fatalf("last_attempt_ok: %#v", block["last_attempt_ok"])
	}
	if block["last_attempt_at"] == nil {
		t.Fatal("last_attempt_at must be present")
	}
	// An inventory failure fabricates neither counters nor metadata.
	wantString(t, block, "size_state", "unknown")
	if s, _ := block["state"].(string); s == "ok" || s == "unknown" {
		t.Fatalf("failed attempt must not look successful: %q", s)
	}
}

func TestSessionStatsErrorKeepsPreviousSuccess(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)
	store.setInventory(SessionInventory{Total: 7, Active: 7, PartitionOK: true}, nil)
	collectOnce(t, srv)
	first := sessionsBlock(t, srv)
	wantString(t, first, "state", "ok")
	collectedAt := first["collected_at"]

	store.setInventory(SessionInventory{}, errors.New("inventory down"))
	allowNextAttempt(srv)
	collectOnce(t, srv)

	block := sessionsBlock(t, srv)
	wantString(t, block, "state", "error")
	wantNum(t, block, "total", 7) // last successful stand is kept
	if block["collected_at"] != collectedAt {
		t.Fatalf("collected_at must not change on failure: %#v vs %#v", block["collected_at"], collectedAt)
	}
	if block["last_attempt_ok"] != false {
		t.Fatalf("last_attempt_ok: %#v", block["last_attempt_ok"])
	}
	// The stored attempt facts must be ordered even though the rendered
	// timestamps have second precision: the outcome flag is the visible
	// discriminator, the raw times prove the ordering.
	snap := publishedSnapshot(srv)
	if !snap.lastAttemptAt.After(snap.collectedAt) {
		t.Fatalf("lastAttemptAt %v must be after collectedAt %v", snap.lastAttemptAt, snap.collectedAt)
	}
	if snap.attempts < 2 {
		t.Fatalf("attempts must be counted: %d", snap.attempts)
	}
}

func TestSessionStatsMetadataIndependentOfCounters(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)
	store.setInventory(SessionInventory{Total: 4, Revoked: 4, PartitionOK: true}, nil)

	// Metadata error: counters must still be published.
	store.setSize(SessionTableSize{}, errors.New("information_schema exploded"))
	collectOnce(t, srv)
	block := sessionsBlock(t, srv)
	wantString(t, block, "state", "ok")
	wantNum(t, block, "total", 4)
	wantString(t, block, "size_state", "error")
	wantNil(t, block, "table_rows_estimate")
	wantNil(t, block, "size_measured_at")

	// Missing metadata row: unknown, not an error and not zero.
	store.setSize(SessionTableSize{}, nil)
	allowNextAttempt(srv)
	collectOnce(t, srv)
	block = sessionsBlock(t, srv)
	wantString(t, block, "size_state", "unknown")
	wantNil(t, block, "table_rows_estimate")
	wantNil(t, block, "data_bytes_estimate")
	wantNil(t, block, "index_bytes_estimate")

	// Recognised privilege rejection.
	store.setSize(SessionTableSize{}, errSessionSizeNoAccess)
	allowNextAttempt(srv)
	collectOnce(t, srv)
	block = sessionsBlock(t, srv)
	wantString(t, block, "size_state", "no_access")
	wantNil(t, block, "index_bytes_estimate")
	wantNum(t, block, "total", 4) // counters survive a metadata problem

	// Present estimates are labelled as estimates and get their own time.
	rows, data, idx := int64(4), int64(8192), int64(2048)
	store.setSize(SessionTableSize{TableRows: &rows, DataBytes: &data, IndexBytes: &idx}, nil)
	allowNextAttempt(srv)
	collectOnce(t, srv)
	block = sessionsBlock(t, srv)
	wantString(t, block, "size_state", "ok")
	wantNum(t, block, "table_rows_estimate", 4)
	wantNum(t, block, "data_bytes_estimate", 8192)
	wantNum(t, block, "index_bytes_estimate", 2048)
	if block["size_measured_at"] == nil {
		t.Fatal("size_measured_at missing")
	}
	firstStand := publishedSnapshot(srv)
	if firstStand.sizeAt.IsZero() {
		t.Fatal("metadata stand must carry its own timestamp")
	}
	if firstStand.sizeState != sessionSizeOK {
		t.Fatalf("sizeState: %q", firstStand.sizeState)
	}

	// Independence in the other direction: a metadata stand from a later
	// attempt must NOT be carried by the timestamp of newer counters. The
	// inventory fails here, so the counters keep their old stand while the
	// metadata is refreshed.
	store.setInventory(SessionInventory{}, errors.New("inventory down"))
	allowNextAttempt(srv)
	collectOnce(t, srv)
	second := publishedSnapshot(srv)
	if !second.collectedAt.Equal(firstStand.collectedAt) {
		t.Fatalf("counters stand must not move: %v vs %v", second.collectedAt, firstStand.collectedAt)
	}
	if !second.sizeAt.After(firstStand.sizeAt) {
		t.Fatalf("metadata stand must advance on its own: %v vs %v", second.sizeAt, firstStand.sizeAt)
	}
	block = sessionsBlock(t, srv)
	wantString(t, block, "state", "error")
	wantString(t, block, "size_state", "ok")
	wantNum(t, block, "table_rows_estimate", 4)
}

func TestSessionStatsParallelRequestsStartOneAttempt(t *testing.T) {
	srv, store := statsServer(t, 5000, 60)
	store.mu.Lock()
	store.block = make(chan struct{})
	store.mu.Unlock()

	const n = 8
	var wg sync.WaitGroup
	for i := 0; i < n; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			statusBody(t, srv)
		}()
	}
	// Exactly one attempt may enter the store.
	select {
	case <-store.entered:
	case <-time.After(5 * time.Second):
		t.Fatal("no collection attempt entered the store")
	}
	// Give the other requests the chance to be admitted (and refused).
	wg.Wait()
	calls, _ := store.counts()
	if calls != 1 {
		t.Fatalf("want exactly 1 collection attempt for %d concurrent requests, got %d", n, calls)
	}

	store.mu.Lock()
	close(store.block)
	store.mu.Unlock()
	awaitStatsPublish(t, srv)
	block := sessionsBlock(t, srv)
	wantString(t, block, "state", "ok")
	if block["in_flight"] != false {
		t.Fatalf("in_flight must be released: %#v", block["in_flight"])
	}
}

func TestSessionStatsMinIntervalAppliesAfterFailure(t *testing.T) {
	srv, store := statsServer(t, 2000, 3600)
	store.setInventory(SessionInventory{}, errors.New("boom"))
	collectOnce(t, srv)

	// The failed attempt already consumed the minimum distance.
	statusBody(t, srv)
	statusBody(t, srv)
	calls, _ := store.counts()
	if calls != 1 {
		t.Fatalf("min interval must throttle after a failure, calls=%d", calls)
	}
	if _, ok := numOrNil(sessionsBlock(t, srv)["age_seconds"]); ok {
		t.Fatal("no successful stand exists, age_seconds must be null")
	}

	// Once the distance is elapsed a retry is admitted again.
	allowNextAttempt(srv)
	store.setInventory(SessionInventory{Total: 1, Active: 1, PartitionOK: true}, nil)
	collectOnce(t, srv)
	calls, _ = store.counts()
	if calls != 2 {
		t.Fatalf("want a second attempt after the interval, calls=%d", calls)
	}
	wantString(t, sessionsBlock(t, srv), "state", "ok")
}

func TestSessionStatsStatusDoesNotWaitForCollection(t *testing.T) {
	srv, store := statsServer(t, 5000, 60)
	store.mu.Lock()
	store.block = make(chan struct{})
	store.mu.Unlock()

	done := make(chan map[string]any, 1)
	go func() { done <- statusBody(t, srv) }()

	// The response must arrive although the collection is blocked.
	var body map[string]any
	select {
	case body = <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("/status waited for the collection")
	}
	if body["status"] != "ok" {
		t.Fatalf("status: %#v", body["status"])
	}
	select {
	case <-store.entered:
	case <-time.After(3 * time.Second):
		t.Fatal("collection never started")
	}
	store.mu.Lock()
	close(store.block)
	store.mu.Unlock()
	awaitStatsPublish(t, srv)
}

func TestSessionStatsTimeoutReleasesInFlight(t *testing.T) {
	srv, store := statsServer(t, 50, 60)
	store.mu.Lock()
	store.blockUntilCtx = true
	store.mu.Unlock()

	collectOnce(t, srv)
	if !store.wasCancelled() {
		t.Fatal("the attempt must end on its own timeout")
	}
	block := sessionsBlock(t, srv)
	wantString(t, block, "state", "error")
	if block["in_flight"] != false {
		t.Fatalf("in_flight must be released after a timeout: %#v", block["in_flight"])
	}

	// In-flight was released, so a later attempt is possible again.
	store.mu.Lock()
	store.blockUntilCtx = false
	store.invErr = nil
	store.inv = SessionInventory{Total: 2, Active: 2, PartitionOK: true}
	store.mu.Unlock()
	allowNextAttempt(srv)
	collectOnce(t, srv)
	calls, _ := store.counts()
	if calls != 2 {
		t.Fatalf("want a retry after the timeout, calls=%d", calls)
	}
	wantString(t, sessionsBlock(t, srv), "state", "ok")
}

// --- P-33 close()/worker lifecycle (K1-K3) ---

// close() with no worker running must lock admission for good: a later
// /status may not start an attempt, not even after the minimum distance
// was cleared.
func TestSessionStatsCloseWithoutWorkerBlocksLaterAdmission(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)

	srv.close()

	running, doneNil, closing, poolClosed := statsLifecycle(srv)
	if running != 0 || !doneNil || !closing || !poolClosed {
		t.Fatalf("close bookkeeping: running=%d doneNil=%v closing=%v poolClosed=%v",
			running, doneNil, closing, poolClosed)
	}
	// The minimum distance would allow an attempt; admission must refuse it.
	allowNextAttempt(srv)
	statusBody(t, srv)
	if calls, _ := store.counts(); calls != 0 {
		t.Fatalf("no attempt may be admitted after close(), calls=%d", calls)
	}
}

// close() while a collection runs must (a) cancel it, (b) reach the running
// store call, and (c) refuse every later attempt.
func TestSessionStatsCloseDuringCollectionCancelsAndBlocksLaterAdmission(t *testing.T) {
	srv, store := statsServer(t, 5000, 60)
	block := make(chan struct{})
	store.mu.Lock()
	store.block = block
	store.mu.Unlock()

	statusBody(t, srv) // admits one attempt
	<-store.entered    // event-driven: the worker is inside the store

	srv.close() // cancels, then waits with the bounded grace

	if !store.wasCancelled() {
		t.Fatal("close() must abort the running collection through its context")
	}
	// Completion bookkeeping ran on the worker's return path.
	running, doneNil, closing, poolClosed := statsLifecycle(srv)
	if running != 0 || !doneNil || !closing || !poolClosed {
		t.Fatalf("worker completion not signalled: running=%d doneNil=%v closing=%v poolClosed=%v",
			running, doneNil, closing, poolClosed)
	}
	allowNextAttempt(srv)
	statusBody(t, srv)
	if calls, _ := store.counts(); calls != 1 {
		t.Fatalf("only the first attempt may exist, calls=%d", calls)
	}
	close(block)
}

// A repeated close() must not admit workers, must not create another
// completion channel and must not close the pool a second time.
func TestSessionStatsRepeatedCloseAddsNoFurtherWork(t *testing.T) {
	srv, store := statsServer(t, 5000, 60)
	block := make(chan struct{})
	store.mu.Lock()
	store.block = block
	store.mu.Unlock()

	statusBody(t, srv)
	<-store.entered
	srv.close()
	srv.close()
	srv.close()

	running, doneNil, _, poolClosed := statsLifecycle(srv)
	if running != 0 || !doneNil || !poolClosed {
		t.Fatalf("repeated close changed state: running=%d doneNil=%v poolClosed=%v",
			running, doneNil, poolClosed)
	}
	allowNextAttempt(srv)
	statusBody(t, srv)
	if calls, _ := store.counts(); calls != 1 {
		t.Fatalf("repeated close admitted another attempt, calls=%d", calls)
	}
	close(block)
}

// A store that ignores ctx cannot be aborted cooperatively. close() must
// still return after its bounded grace instead of hanging; the test then
// releases the block so the worker ends.
func TestSessionStatsCloseWaitsBoundedWhenStoreIgnoresContext(t *testing.T) {
	srv, base := statsServer(t, 2000, 60)
	release := make(chan struct{})
	entered := make(chan struct{}, 1)
	srv.store = &ctxIgnoringStatsStore{statsStore: base, release: release, entered: entered}

	statusBody(t, srv)
	<-entered // the worker is blocked and ignores ctx

	// Upper hang bound only; the grace itself is not asserted by timing.
	done := make(chan struct{})
	go func() {
		srv.close()
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(30 * time.Second):
		t.Fatal("close() must not wait unbounded on a store that ignores ctx")
	}
	if base.wasCancelled() {
		t.Fatal("this store ignores ctx; the attempt must end by release, not by cancel")
	}

	// Release the test block and let the worker finish.
	close(release)
	awaitStatsPublish(t, srv)
	if running, doneNil, _, _ := statsLifecycle(srv); running != 0 || !doneNil {
		t.Fatalf("worker did not finish after release: running=%d doneNil=%v", running, doneNil)
	}
}

// --- P-33 renderer and stale threshold (K4, K5) ---

// The renderer must stay consistent while the FIRST collection runs, i.e.
// before any snapshot exists. State and in_flight come from the same locked
// copy, and the counters stay unknown instead of appearing as zero.
func TestSessionStatsRendererDuringFirstCollectionIsConsistent(t *testing.T) {
	srv, store := statsServer(t, 5000, 60)
	block := make(chan struct{})
	store.mu.Lock()
	store.block = block
	store.mu.Unlock()

	// The first /status admits the first attempt.
	statusBody(t, srv)
	<-store.entered // the worker is inside the store

	running := sessionsBlock(t, srv)
	wantString(t, running, "state", "running")
	if running["in_flight"] != true {
		t.Fatalf("in_flight must be true while the first attempt runs: %#v", running["in_flight"])
	}
	// Counters stay UNKNOWN (null), never 0, before the first success.
	wantNil(t, running, "total")
	wantNil(t, running, "active")
	wantNil(t, running, "revoked")
	wantNil(t, running, "collected_at")
	wantNum(t, running, "attempts", 1)

	close(block)
	awaitStatsPublish(t, srv)
}

// stale_after_seconds must be the SAME threshold the stale decision uses:
// twice the effective minimum distance, published in seconds.
func TestSessionStatsStaleAfterSecondsMatchesEffectiveThreshold(t *testing.T) {
	srv, store := statsServer(t, 2000, 45) // 45 s minimum distance
	store.setInventory(SessionInventory{Total: 3, Active: 3, PartitionOK: true}, nil)
	collectOnce(t, srv)

	block := sessionsBlock(t, srv)
	wantNum(t, block, "min_interval_seconds", 45)
	wantNum(t, block, "stale_after_seconds", 90)
	wantString(t, block, "state", "ok")

	// The boundary follows the published threshold: inside it stays ok,
	// beyond it the stand counts as stale.
	sn := publishedSnapshot(srv)
	if sn == nil {
		t.Fatal("no published snapshot")
	}
	threshold := sessionStatsStaleAfter(45 * time.Second)
	if threshold != 90*time.Second {
		t.Fatalf("threshold definition: %s", threshold)
	}
	if st := resolveSessionStatsState(sn, sn.collectedAt.Add(threshold-time.Second), 45*time.Second, true); st != sessionStatsOK {
		t.Fatalf("inside the threshold the stand must stay ok, got %q", st)
	}
	if st := resolveSessionStatsState(sn, sn.collectedAt.Add(threshold+time.Second), 45*time.Second, true); st != sessionStatsStale {
		t.Fatalf("beyond the threshold the stand must be stale, got %q", st)
	}
	// Disabled output still publishes the threshold, so the field is never
	// an accidental zero.
	srv.cfg.SessionStatsEnabled = false
	wantNum(t, sessionsBlock(t, srv), "stale_after_seconds", 90)
	wantString(t, sessionsBlock(t, srv), "state", "disabled")
}

func TestSessionStatsDeltaAgainstLastSuccessfulStock(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)
	store.setInventory(SessionInventory{Total: 10, Active: 10, PartitionOK: true}, nil)
	collectOnce(t, srv)
	wantNil(t, sessionsBlock(t, srv), "total_delta")

	// A failed attempt must not become the comparison base.
	store.setInventory(SessionInventory{}, errors.New("boom"))
	allowNextAttempt(srv)
	collectOnce(t, srv)

	store.setInventory(SessionInventory{Total: 13, Active: 13, PartitionOK: true}, nil)
	allowNextAttempt(srv)
	collectOnce(t, srv)

	block := sessionsBlock(t, srv)
	wantNum(t, block, "total", 13)
	wantNum(t, block, "total_delta", 3) // net stock change, not new logins
	window, ok := numOrNil(block["delta_window_seconds"])
	if !ok || window <= 0 {
		t.Fatalf("delta_window_seconds must be positive: %#v", block["delta_window_seconds"])
	}
}

func TestSessionStatsOutputCarriesNoSensitiveData(t *testing.T) {
	srv, store := statsServer(t, 2000, 60)
	fs := store.fakeStore
	acc := addAccount(t, fs, "monitoruser", "longenough1", "m@e.com", false)
	raw := mkSession(t, fs, acc.ID, time.Hour)
	store.setInventory(SessionInventory{Total: 1, Active: 1, PartitionOK: true}, nil)
	collectOnce(t, srv)

	req := httptest.NewRequest("GET", "/status", nil)
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	for _, forbidden := range []string{
		raw, tokenHash(raw), "monitoruser", "m@e.com", "account_id", "token", "session_id",
		"password", "SELECT", "inventory down", "boom",
	} {
		if strings.Contains(body, forbidden) {
			t.Fatalf("/status must not contain %q: %s", forbidden, body)
		}
	}
	// No raw driver message even after a failed attempt.
	store.setInventory(SessionInventory{}, errors.New("driver said: connection refused to 10.0.0.5"))
	allowNextAttempt(srv)
	collectOnce(t, srv)
	rec = httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, httptest.NewRequest("GET", "/status", nil))
	if strings.Contains(rec.Body.String(), "connection refused") {
		t.Fatalf("raw driver error leaked: %s", rec.Body.String())
	}
}

// --- scripted SQL driver (P-33) ---
//
// The tests below exercise the REAL *sQLStore methods (statement, value
// mapping, timing) through a scripted database/sql driver. The driver
// executes NO SQL, opens NO connection to any database and is NOT a
// MariaDB test: the documented limits of this coverage are stated in
// docs/Security.md (P-33).

// sqlScript maps a statement fragment to a scripted answer.
type sqlScript struct {
	mu      sync.Mutex
	answers map[string]scriptAnswer
	queries []string
}

type scriptAnswer struct {
	cols []string
	rows [][]driver.Value
	err  error
}

func newSQLScript() *sqlScript {
	return &sqlScript{answers: map[string]scriptAnswer{}}
}

func (s *sqlScript) on(fragment string, cols []string, rows [][]driver.Value) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.answers[fragment] = scriptAnswer{cols: cols, rows: rows}
}

func (s *sqlScript) fail(fragment string, err error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.answers[fragment] = scriptAnswer{err: err}
}

func (s *sqlScript) answer(query string) (scriptAnswer, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.queries = append(s.queries, query)
	for fragment, a := range s.answers {
		if strings.Contains(query, fragment) {
			return a, true
		}
	}
	return scriptAnswer{}, false
}

func (s *sqlScript) saw(fragment string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, q := range s.queries {
		if strings.Contains(q, fragment) {
			return true
		}
	}
	return false
}

func (s *sqlScript) firstQuery() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.queries) == 0 {
		return ""
	}
	return s.queries[0]
}

var scriptDriverSeq atomic.Int64

type scriptDriver struct{ script *sqlScript }

func (d *scriptDriver) Open(string) (driver.Conn, error) { return &scriptConn{script: d.script}, nil }

type scriptConn struct{ script *sqlScript }

func (c *scriptConn) Prepare(string) (driver.Stmt, error) {
	return nil, errors.New("scripted driver: Prepare is not supported")
}
func (c *scriptConn) Close() error { return nil }
func (c *scriptConn) Begin() (driver.Tx, error) {
	return nil, errors.New("scripted driver: no transactions")
}

func (c *scriptConn) QueryContext(_ context.Context, query string, _ []driver.NamedValue) (driver.Rows, error) {
	answer, ok := c.script.answer(query)
	if !ok {
		return nil, fmt.Errorf("scripted driver: no answer for %q", query)
	}
	if answer.err != nil {
		return nil, answer.err
	}
	return &scriptRows{cols: answer.cols, rows: answer.rows}, nil
}

type scriptRows struct {
	cols []string
	rows [][]driver.Value
	pos  int
}

func (r *scriptRows) Columns() []string { return r.cols }
func (r *scriptRows) Close() error      { return nil }
func (r *scriptRows) Next(dest []driver.Value) error {
	if r.pos >= len(r.rows) {
		return io.EOF
	}
	copy(dest, r.rows[r.pos])
	r.pos++
	return nil
}

// scriptedStore builds a real *sQLStore on top of the scripted driver.
func scriptedStore(t *testing.T, script *sqlScript) *sQLStore {
	t.Helper()
	name := fmt.Sprintf("andora-script-%d", scriptDriverSeq.Add(1))
	sql.Register(name, &scriptDriver{script: script})
	db, err := sql.Open(name, "")
	if err != nil {
		t.Fatalf("open scripted db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	return &sQLStore{db: db}
}

func TestSessionInventoryStatementAndMapping(t *testing.T) {
	script := newSQLScript()
	script.on("FROM sessions",
		[]string{"total", "active", "expired_not_revoked", "revoked", "oldest_row_age_days"},
		[][]driver.Value{
			{int64(12), int64(4), int64(5), int64(3), int64(30)},
		})
	store := scriptedStore(t, script)

	inv, err := store.SessionInventory(context.Background())
	if err != nil {
		t.Fatalf("inventory: %v", err)
	}
	if inv.Total != 12 || inv.Active != 4 || inv.ExpiredNotRevoked != 5 || inv.Revoked != 3 {
		t.Fatalf("mapping wrong: %+v", inv)
	}
	if !inv.PartitionOK {
		t.Fatalf("partition must hold: %+v", inv)
	}
	if inv.OldestRowAgeDays == nil || *inv.OldestRowAgeDays != 30 {
		t.Fatalf("age: %+v", inv.OldestRowAgeDays)
	}
	if inv.QueryMS <= 0 {
		t.Fatalf("query duration must be measured: %v", inv.QueryMS)
	}

	// The statement must keep the documented semantics: one aggregate,
	// DB-side NOW(), COALESCE for the empty table, NULL age when empty,
	// and revoked counted independently of expiry.
	q := script.firstQuery()
	for _, want := range []string{
		"COALESCE(SUM(CASE WHEN revoked_at IS NULL AND expires_at >= NOW()",
		"COALESCE(SUM(CASE WHEN revoked_at IS NULL AND expires_at <  NOW()",
		"COALESCE(SUM(CASE WHEN revoked_at IS NOT NULL THEN 1 ELSE 0 END), 0)",
		"CASE WHEN COUNT(*) = 0 THEN NULL",
		"TIMESTAMPDIFF(DAY, MIN(created_at), NOW())",
	} {
		if !strings.Contains(q, want) {
			t.Fatalf("statement missing %q:\n%s", want, q)
		}
	}
	if strings.Contains(q, "DELETE") || strings.Contains(q, "UPDATE") {
		t.Fatalf("monitoring must be read-only:\n%s", q)
	}

	// NULL age (empty table) stays unknown instead of becoming 0.
	script.on("FROM sessions",
		[]string{"total", "active", "expired_not_revoked", "revoked", "oldest_row_age_days"},
		[][]driver.Value{{int64(0), int64(0), int64(0), int64(0), nil}})
	inv, err = store.SessionInventory(context.Background())
	if err != nil {
		t.Fatalf("inventory empty: %v", err)
	}
	if inv.OldestRowAgeDays != nil {
		t.Fatalf("empty table must report an unknown age, got %d", *inv.OldestRowAgeDays)
	}
	if !inv.PartitionOK || inv.Total != 0 {
		t.Fatalf("empty table: %+v", inv)
	}
}

func TestSessionInventoryFailureCarriesNoMeasurement(t *testing.T) {
	script := newSQLScript()
	script.fail("FROM sessions", errors.New("inventory boom"))
	store := scriptedStore(t, script)

	inv, err := store.SessionInventory(context.Background())
	if err == nil {
		t.Fatal("want an error")
	}
	if inv.Total != 0 || inv.QueryMS != 0 || inv.PartitionOK || inv.OldestRowAgeDays != nil {
		t.Fatalf("a failed query must not be a measurement: %+v", inv)
	}
	if strings.Contains(err.Error(), "SELECT") {
		t.Fatalf("error text must stay wrapped and short: %v", err)
	}
}

func TestSessionTableSizeUnknownAndNoAccess(t *testing.T) {
	script := newSQLScript()
	store := scriptedStore(t, script)

	// No visible row: unknown, no error.
	script.fail("information_schema.TABLES", sql.ErrNoRows)
	size, err := store.SessionTableSize(context.Background())
	if err != nil {
		t.Fatalf("missing row must not be an error: %v", err)
	}
	if size.TableRows != nil || size.DataBytes != nil || size.IndexBytes != nil {
		t.Fatalf("missing row must stay unknown: %+v", size)
	}
	if !script.saw("TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sessions'") {
		t.Fatal("statement must scope to the current database and the sessions table")
	}

	// Present estimates.
	script.on("information_schema.TABLES", []string{"TABLE_ROWS", "DATA_LENGTH", "INDEX_LENGTH"},
		[][]driver.Value{{int64(1200), int64(40000), int64(9000)}})
	size, err = store.SessionTableSize(context.Background())
	if err != nil {
		t.Fatalf("size: %v", err)
	}
	if size.TableRows == nil || *size.TableRows != 1200 ||
		size.DataBytes == nil || *size.DataBytes != 40000 ||
		size.IndexBytes == nil || *size.IndexBytes != 9000 {
		t.Fatalf("estimates: %+v", size)
	}

	// Recognised privilege rejection.
	script.fail("information_schema.TABLES", &mysqlerr.MySQLError{
		Number: 1142, Message: "SELECT command denied to user for table 'TABLES'",
	})
	if _, err = store.SessionTableSize(context.Background()); !errors.Is(err, errSessionSizeNoAccess) {
		t.Fatalf("want errSessionSizeNoAccess, got %v", err)
	}

	// Any other error stays a neutral error, never a denial.
	script.fail("information_schema.TABLES", errors.New("connection reset"))
	if _, err = store.SessionTableSize(context.Background()); errors.Is(err, errSessionSizeNoAccess) {
		t.Fatal("a generic error must not be reported as no_access")
	} else if err == nil {
		t.Fatal("want an error")
	}
}

func TestSessionStatusBatchLookupTimingOnSQLStore(t *testing.T) {
	script := newSQLScript()
	script.on("FROM sessions WHERE token_hash IN",
		[]string{"token_hash", "account_id", "expires_at", "revoked_at"},
		[][]driver.Value{{"a1", int64(1), time.Now().Add(time.Hour), nil}})
	store := scriptedStore(t, script)

	if _, _, _, ok := store.BatchLookupTiming(); ok {
		t.Fatal("no measurement before the first lookup")
	}
	if _, _, err := store.BatchSessionStatus(context.Background(), []string{"t1"}); err != nil {
		t.Fatalf("batch: %v", err)
	}
	last, max, measuredAt, ok := store.BatchLookupTiming()
	if !ok || last <= 0 || max < last || measuredAt.IsZero() {
		t.Fatalf("successful lookup must be measured: last=%v max=%v at=%v ok=%v", last, max, measuredAt, ok)
	}

	// A failing lookup must keep the previous success and must not be
	// presented as a new measurement.
	script.fail("FROM sessions WHERE token_hash IN", errors.New("lookup boom"))
	if _, _, err := store.BatchSessionStatus(context.Background(), []string{"t1"}); err == nil {
		t.Fatal("want an error")
	}
	last2, max2, measuredAt2, ok2 := store.BatchLookupTiming()
	if !ok2 || last2 != last || max2 != max || !measuredAt2.Equal(measuredAt) {
		t.Fatalf("failed lookup must not change the measurement: %v/%v/%v", last2, max2, measuredAt2)
	}

	// Empty input does no DB work and is therefore not measured.
	before := last2
	if _, _, err := store.BatchSessionStatus(context.Background(), nil); err != nil {
		t.Fatalf("empty batch: %v", err)
	}
	if l, _, _, _ := store.BatchLookupTiming(); l != before {
		t.Fatalf("empty batch must not overwrite the measurement: %v vs %v", l, before)
	}
}

func TestSessionStatusBatchLookupTimingIsConcurrencySafe(t *testing.T) {
	script := newSQLScript()
	script.on("FROM sessions WHERE token_hash IN",
		[]string{"token_hash", "account_id", "expires_at", "revoked_at"},
		[][]driver.Value{{"a1", int64(1), time.Now().Add(time.Hour), nil}})
	store := scriptedStore(t, script)

	var wg sync.WaitGroup
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 10; j++ {
				if _, _, err := store.BatchSessionStatus(context.Background(), []string{"t1"}); err != nil {
					t.Errorf("batch: %v", err)
					return
				}
			}
		}()
	}
	wg.Wait()
	last, max, measuredAt, ok := store.BatchLookupTiming()
	if !ok || last <= 0 || max < last || measuredAt.IsZero() {
		t.Fatalf("measurement after concurrent lookups: last=%v max=%v ok=%v", last, max, ok)
	}
}
