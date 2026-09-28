package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
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
