package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
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
