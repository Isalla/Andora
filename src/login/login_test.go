package main

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

const (
	testSvcID = "login-service"
	testSvcPw = "test-secret"
)

// fakeAuth is a stub Auth/API-Service: it verifies the HMAC signature
// scheme (same construction as src/api/auth.go) and answers canned
// payloads. seen records the decoded request bodies per path.
type fakeAuth struct {
	t         *testing.T
	seen      map[string][]map[string]any
	verify    map[string]any
	parStatus int
	parBody   map[string]any
}

func newFakeAuth(t *testing.T) (*fakeAuth, *httptest.Server) {
	f := &fakeAuth{t: t, seen: map[string][]map[string]any{}}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		var decoded map[string]any
		_ = json.Unmarshal(body, &decoded)
		f.seen[r.URL.Path] = append(f.seen[r.URL.Path], decoded)

		if r.Header.Get("X-Andora-Service") != testSvcID {
			t.Errorf("auth stub: want service %q, got %q", testSvcID, r.Header.Get("X-Andora-Service"))
		}
		ts, err := strconv.ParseInt(r.Header.Get("X-Andora-Timestamp"), 10, 64)
		if err != nil || abs(time.Now().Unix()-ts) > 300 {
			t.Errorf("auth stub: bad timestamp %q", r.Header.Get("X-Andora-Timestamp"))
		}
		sum := sha256.Sum256(body)
		payload := fmt.Sprintf("%s\n%s\n%s\n%d\n%s",
			r.Method, r.URL.Path, r.URL.RawQuery, ts, hex.EncodeToString(sum[:]))
		mac := hmac.New(sha256.New, []byte(testSvcPw))
		mac.Write([]byte(payload))
		if r.Header.Get("X-Andora-Signature") != hex.EncodeToString(mac.Sum(nil)) {
			t.Errorf("auth stub: bad signature for %s", r.URL.Path)
		}

		write := func(v any) {
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(v)
		}
		switch r.URL.Path {
		case "/auth/verify":
			if f.verify != nil {
				write(f.verify)
				return
			}
			write(map[string]any{"valid": true, "account_id": 7, "session_id": "sess-1", "expires_at": "2030-01-01T00:00:00Z"})
		case "/session/validate":
			if decoded["session_id"] == "good" {
				write(map[string]any{"valid": true, "account_id": 7})
			} else {
				write(map[string]any{"valid": false})
			}
		case "/realms":
			write(map[string]any{"realms": []any{
				map[string]any{"id": 1, "name": "DE-1", "language": "de", "region": "eu", "enabled": true, "transfer_policy": "open"},
				map[string]any{"id": 2, "name": "DE-2", "language": "de", "region": "eu", "enabled": false, "transfer_policy": "closed"},
			}})
		case "/handoff/create":
			write(map[string]any{"handoff_token": "ho-1", "expires_at": "2030-01-01T00:01:00Z"})
		case "/session/revoke":
			write(map[string]any{"revoked": true})
		case "/parental/status", "/parental/setup", "/parental/update", "/parental/remove",
			"/parental/pin/change", "/parental/pin/verify", "/parental/periods",
			"/parental/periods/add", "/parental/periods/remove",
			"/parental/exceptions/add", "/parental/exceptions/remove":
			if f.parStatus != 0 {
				w.WriteHeader(f.parStatus)
				if f.parBody != nil {
					write(f.parBody)
				}
				return
			}
			write(map[string]any{"canned": true})
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	return f, srv
}

func abs(n int64) int64 {
	if n < 0 {
		return -n
	}
	return n
}

func testServer(t *testing.T, authURL string) *Server {
	t.Helper()
	cfg := &Config{
		Port: 0, AuthAPIURL: authURL, ServiceID: testSvcID, Secret: testSvcPw,
		RealmWS:        map[int]string{1: "ws://realm1:3001"},
		RealmHTTP:      map[int]string{1: "http://realm1:3002"},
		AuthAPITimeout: 2 * time.Second, RateLimitBurst: 100, RateLimitPerMin: 1000,
	}
	return newServer(cfg)
}

func doPost(t *testing.T, srv *Server, path, body string) (int, map[string]any) {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
	req.RemoteAddr = "127.0.0.1:1"
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	var out map[string]any
	_ = json.Unmarshal(rec.Body.Bytes(), &out)
	return rec.Code, out
}

func TestLoginSuccess(t *testing.T) {
	_, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	code, out := doPost(t, srv, "/login", `{"username":"alice","password":"secret123"}`)
	if code != 200 || out["valid"] != true || out["account_id"] != float64(7) {
		t.Fatalf("login: code=%d body=%v", code, out)
	}
}

func TestLoginValidation(t *testing.T) {
	_, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	if code, _ := doPost(t, srv, "/login", `{"username":"alice"}`); code != 400 {
		t.Fatalf("missing password: want 400, got %d", code)
	}
	if code, _ := doPost(t, srv, "/login", `{"password":"x"}`); code != 400 {
		t.Fatalf("missing identifier: want 400, got %d", code)
	}
}

func TestLoginAuthDown(t *testing.T) {
	srv := testServer(t, "http://127.0.0.1:1")
	code, out := doPost(t, srv, "/login", `{"username":"alice","password":"secret123"}`)
	if code != 503 || out["error"] != "auth service unavailable" {
		t.Fatalf("auth down: code=%d body=%v", code, out)
	}
}

func TestLogout(t *testing.T) {
	_, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	code, out := doPost(t, srv, "/logout", `{"session_id":"good"}`)
	if code != 200 || out["revoked"] != true {
		t.Fatalf("logout: code=%d body=%v", code, out)
	}
}

func TestRealms(t *testing.T) {
	_, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)

	req := httptest.NewRequest(http.MethodGet, "/realms?session_id=good", nil)
	req.RemoteAddr = "127.0.0.1:1"
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	var out map[string]any
	_ = json.Unmarshal(rec.Body.Bytes(), &out)
	realms, _ := out["realms"].([]any)
	if rec.Code != 200 || len(realms) != 2 {
		t.Fatalf("realms: code=%d body=%v", rec.Code, out)
	}
	if realms[0].(map[string]any)["http_url"] != "http://realm1:3002" || realms[1].(map[string]any)["http_url"] != "" {
		t.Fatalf("realm HTTP mapping missing or inferred: %v", realms)
	}

	req = httptest.NewRequest(http.MethodGet, "/realms?session_id=bad", nil)
	req.RemoteAddr = "127.0.0.1:1"
	rec = httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	if rec.Code != 401 {
		t.Fatalf("bad session: want 401, got %d", rec.Code)
	}
}

func TestHandoff(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)

	code, out := doPost(t, srv, "/handoff", `{"session_id":"good","realm_id":1}`)
	if code != 200 || out["handoff_token"] != "ho-1" || out["ws_url"] != "ws://realm1:3001" || out["realm_id"] != float64(1) {
		t.Fatalf("handoff: code=%d body=%v", code, out)
	}
	// account_id must come from the validated session (7), not the client.
	got := f.seen["/handoff/create"]
	if len(got) != 1 || got[0]["account_id"] != float64(7) || got[0]["realm_id"] != float64(1) {
		t.Fatalf("handoff/create saw wrong body: %v", got)
	}

	if code, _ := doPost(t, srv, "/handoff", `{"session_id":"good","realm_id":99}`); code != 404 {
		t.Fatalf("unknown realm: want 404, got %d", code)
	}
	if code, _ := doPost(t, srv, "/handoff", `{"session_id":"good","realm_id":2}`); code != 403 {
		t.Fatalf("disabled realm: want 403, got %d", code)
	}
	if code, _ := doPost(t, srv, "/handoff", `{"session_id":"bad","realm_id":1}`); code != 401 {
		t.Fatalf("bad session: want 401, got %d", code)
	}
}

func TestParseRealmWS(t *testing.T) {
	m, err := parseRealmWS("1=ws://a:3001, 2 = ws://b:3001")
	if err != nil || m[1] != "ws://a:3001" || m[2] != "ws://b:3001" {
		t.Fatalf("parse: %v %v", m, err)
	}
	for _, bad := range []string{"abc", "1=", "=x", "0=ws://a", "1=a,1=b", "x=y"} {
		if _, err := parseRealmWS(bad); err == nil {
			t.Fatalf("parse %q: want error", bad)
		}
	}
}

func TestRealmHTTPConfig(t *testing.T) {
	path := filepath.Join(t.TempDir(), "login.env")
	for _, raw := range []string{"", "1=http://127.0.0.1:3002", "1=https://realm.example/base/"} {
		body := "AUTHAPI_URL=http://127.0.0.1:8080\nLOGIN_SERVICE_ID=test\nLOGIN_SERVICE_SECRET=test\nREALM_HTTP_URLS=" + raw + "\n"
		if err := os.WriteFile(path, []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
		cfg, err := loadConfig(path)
		if err != nil || (raw != "" && cfg.RealmHTTP[1] == "") {
			t.Fatalf("valid HTTP mapping rejected: %v", err)
		}
	}
	for _, raw := range []string{"1=ws://realm:3001", "1=http://user:secret@realm", "1=http://realm?secret=value", "1=http://realm#fragment", "1=http://realm,1=http://other"} {
		body := "AUTHAPI_URL=http://127.0.0.1:8080\nLOGIN_SERVICE_ID=test\nLOGIN_SERVICE_SECRET=test\nREALM_HTTP_URLS=" + raw + "\n"
		if err := os.WriteFile(path, []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := loadConfig(path); err == nil {
			t.Fatal("invalid HTTP mapping accepted")
		}
	}
}

func TestHealth(t *testing.T) {
	srv := testServer(t, "http://127.0.0.1:1")
	for _, p := range []string{"/health", "/status"} {
		req := httptest.NewRequest(http.MethodGet, p, nil)
		req.RemoteAddr = "127.0.0.1:1"
		rec := httptest.NewRecorder()
		srv.handler().ServeHTTP(rec, req)
		if rec.Code != 200 {
			t.Fatalf("%s: want 200, got %d", p, rec.Code)
		}
	}
}

// --- parental proxy tests (POST /parental) ---

func hasKey(m map[string]any, k string) bool {
	_, ok := m[k]
	return ok
}

func hasPath(seen map[string][]map[string]any, k string) bool {
	_, ok := seen[k]
	return ok
}

func TestParentalStatusPassthrough(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	code, out := doPost(t, srv, "/parental", `{"session_id":"good","action":"status"}`)
	if code != 200 || out["canned"] != true {
		t.Fatalf("parental status: code=%d body=%v", code, out)
	}
	seen := f.seen["/parental/status"]
	if seen == nil || len(seen) != 1 || seen[0]["account_id"] != float64(7) {
		t.Fatalf("forwarded account_id wrong: %v", seen)
	}
}

func TestParentalAccountOverride(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	code, _ := doPost(t, srv, "/parental",
		`{"session_id":"good","action":"setup","account_id":999,"parent_pin":"1234","evil":"x"}`)
	if code != 200 {
		t.Fatalf("setup: code=%d", code)
	}
	seen := f.seen["/parental/setup"]
	if seen == nil || len(seen) != 1 {
		t.Fatalf("setup not forwarded once: %v", seen)
	}
	var fwd map[string]any = seen[0]
	if fwd["account_id"] != float64(7) || hasKey(fwd, "evil") || hasKey(fwd, "session_id") {
		t.Fatalf("forwarded body wrong: %v", fwd)
	}
}

func TestParentalUnknownAction(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	code, out := doPost(t, srv, "/parental", `{"session_id":"good","action":"notifications"}`)
	if code != 400 || out["error"] != "unknown action" {
		t.Fatalf("unknown action: code=%d body=%v", code, out)
	}
	if hasPath(f.seen, "/session/validate") || hasPath(f.seen, "/parental/notifications") {
		t.Fatalf("auth called for unknown action")
	}
}

func TestParentalInvalidSession(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	code, out := doPost(t, srv, "/parental", `{"session_id":"bad","action":"status"}`)
	if code != 401 || out["error"] != "invalid session" {
		t.Fatalf("invalid session: code=%d body=%v", code, out)
	}
	if hasPath(f.seen, "/parental/status") {
		t.Fatalf("parental called with invalid session")
	}
}

func TestParentalPinMismatch(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	f.parStatus = 401
	code, out := doPost(t, srv, "/parental", `{"session_id":"good","action":"pin_verify","parent_pin":"9999"}`)
	if code != 401 || out["error"] != "parent pin does not match" {
		t.Fatalf("pin mismatch: code=%d body=%v", code, out)
	}
}

func TestParentalConflict(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	f.parStatus = 409
	code, _ := doPost(t, srv, "/parental",
		`{"session_id":"good","action":"periods_add","parent_pin":"1234","starts_at":"2026-10-01","ends_at":"2026-10-08"}`)
	if code != 409 {
		t.Fatalf("conflict: code=%d", code)
	}
}

func TestParentalFieldFilter(t *testing.T) {
	f, auth := newFakeAuth(t)
	defer auth.Close()
	srv := testServer(t, auth.URL)
	code, _ := doPost(t, srv, "/parental",
		`{"session_id":"good","action":"exceptions_add","parent_pin":"1234","date":"2026-10-20","extra_minutes":60,"handoff_token":"steal"}`)
	if code != 200 {
		t.Fatalf("exceptions_add: code=%d", code)
	}
	seen := f.seen["/parental/exceptions/add"]
	if seen == nil || len(seen) != 1 {
		t.Fatalf("exceptions_add not forwarded once: %v", seen)
	}
	var fwd map[string]any = seen[0]
	if hasKey(fwd, "handoff_token") || hasKey(fwd, "session_id") || fwd["account_id"] != float64(7) {
		t.Fatalf("forwarded body wrong: %v", fwd)
	}
}

func TestParentalAuthDown(t *testing.T) {
	srv := testServer(t, "http://127.0.0.1:9")
	code, out := doPost(t, srv, "/parental", `{"session_id":"good","action":"status"}`)
	if code != 503 || out["error"] != "auth service unavailable" {
		t.Fatalf("auth down: code=%d body=%v", code, out)
	}
}

// --- H-2: the rate-limit key must not be multiplied by attacker-chosen paths ---

// rawGet runs an unauthenticated GET with an explicit client IP through the
// full handler chain (pre-auth rate limit, then routing).
func rawGet(t *testing.T, srv *Server, remoteAddr, path string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.RemoteAddr = remoteAddr
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	return rec
}

// heldKeys counts the limiter's live key entries (test seam: same package).
func heldKeys(t *testing.T, srv *Server) int {
	t.Helper()
	srv.rl.mu.Lock()
	defer srv.rl.mu.Unlock()
	return len(srv.rl.hits)
}

// (2) varying unknown paths from the same IP must NOT bypass the per-IP
// limit and must NOT mint one counter per unique path.
func TestRateLimitPreAuthUnknownPathBypass(t *testing.T) {
	srv := testServer(t, "http://127.0.0.1:1")
	ip := "10.0.0.9:80"
	burst := srv.cfg.RateLimitBurst
	var last *httptest.ResponseRecorder
	for i := 0; i <= burst; i++ {
		last = rawGet(t, srv, ip, "/nope/"+strconv.Itoa(i))
	}
	if last.Code != http.StatusTooManyRequests {
		t.Fatalf("varying unknown paths must still hit the per-IP limit, got %d", last.Code)
	}
	if last.Header().Get("Retry-After") == "" {
		t.Fatal("Retry-After header missing")
	}
	if n := heldKeys(t, srv); n != 1 {
		t.Fatalf("expected exactly 1 rate-limit key for %d unknown paths, got %d", burst+1, n)
	}
}

// (3) legitimate endpoint groups stay correctly separated.
func TestRateLimitPreAuthEndpointSeparation(t *testing.T) {
	srv := testServer(t, "http://127.0.0.1:1")
	ip := "10.0.0.7:80"
	burst := srv.cfg.RateLimitBurst
	for i := 0; i < burst; i++ {
		if r := rawGet(t, srv, ip, "/status"); r.Code != http.StatusOK {
			t.Fatalf("status request %d: want 200, got %d", i, r.Code)
		}
	}
	if r := rawGet(t, srv, ip, "/status"); r.Code != http.StatusTooManyRequests {
		t.Fatalf("status bucket exhausted: want 429, got %d", r.Code)
	}
	if r := rawGet(t, srv, ip, "/health"); r.Code != http.StatusOK {
		t.Fatalf("separate endpoint group must not be blocked by /status: got %d", r.Code)
	}
}

// (4) different client IPs keep independent limits.
func TestRateLimitPreAuthDistinctIPs(t *testing.T) {
	srv := testServer(t, "http://127.0.0.1:1")
	burst := srv.cfg.RateLimitBurst
	var last *httptest.ResponseRecorder
	for i := 0; i <= burst; i++ {
		last = rawGet(t, srv, "10.0.0.1:80", "/x")
	}
	if last.Code != http.StatusTooManyRequests {
		t.Fatalf("IP A unknown bucket: want 429, got %d", last.Code)
	}
	if r := rawGet(t, srv, "10.0.0.2:80", "/x"); r.Code == http.StatusTooManyRequests {
		t.Fatalf("IP B must not inherit IP A's limit")
	}
	if n := heldKeys(t, srv); n != 2 {
		t.Fatalf("expected 2 per-IP keys, got %d", n)
	}
}
