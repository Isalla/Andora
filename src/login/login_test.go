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
	t      *testing.T
	seen   map[string][]map[string]any
	verify map[string]any
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
