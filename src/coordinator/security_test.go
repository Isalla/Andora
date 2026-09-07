package main

import (
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

func newTestServer(t *testing.T, envExtra string) *Server {
	t.Helper()
	cfg := loadEnvString(t, minimalEnv+envExtra)
	if cfg.TLSCertFile != "" || cfg.TLSKeyFile != "" {
		t.Fatal("test config must not enable TLS")
	}
	cfg.DataDir = filepath.Join(t.TempDir(), "data")
	store := newFileStore(cfg.DataDir)
	if err := store.Init(); err != nil {
		t.Fatal(err)
	}
	return newServer(cfg, store)
}

// signedRequest builds a request exactly like a realm client would.
func signedRequest(cred RealmCred, method, path, rawQuery string, ts int64, body string) *http.Request {
	req := httptest.NewRequest(method, path+"?"+rawQuery, strings.NewReader(body))
	req.Header.Set("X-Andora-Service", cred.ServiceID)
	req.Header.Set("X-Andora-Timestamp", strconv.FormatInt(ts, 10))
	req.Header.Set("X-Andora-Signature", signPayload(cred.Secret, method, path, rawQuery, ts, []byte(body)))
	return req
}

func TestSignPayloadVector(t *testing.T) {
	got := signPayload("sec", "POST", "/v1/jobs", "", 1712345678, []byte(`{"a":1}`))
	if len(got) != 64 {
		t.Fatalf("signature length = %d, want 64 (hex sha256)", len(got))
	}
}

func TestSignPayloadSensitiveToEveryField(t *testing.T) {
	body := []byte(`{"job_type":"player"}`)
	base := signPayload("sec", "POST", "/v1/jobs", "", 1712345678, body)
	tamper := []struct {
		secret, method, path, query string
		ts                          int64
		body                        []byte
	}{
		{"sec", "POST", "/v1/jobs", "", 1712345678, []byte(`{"job_type":"craft"}`)},
		{"other", "POST", "/v1/jobs", "", 1712345678, body},
		{"sec", "GET", "/v1/jobs", "", 1712345678, body},
		{"sec", "POST", "/v1/jobs/x", "", 1712345678, body},
		{"sec", "POST", "/v1/jobs", "realm_id=de1", 1712345678, body},
		{"sec", "POST", "/v1/jobs", "", 1712345679, body},
	}
	for i, c := range tamper {
		if got := signPayload(c.secret, c.method, c.path, c.query, c.ts, c.body); got == base {
			t.Errorf("tamper case %d must differ", i)
		}
	}
}

func TestAuthorizeOK(t *testing.T) {
	s := newTestServer(t, "")
	cred := s.cfg.Realms["de1"]
	body := `{"job_type":"player","text":"hallo"}`
	r := signedRequest(cred, "POST", "/v1/jobs", "", time.Now().Unix(), body)
	w := httptest.NewRecorder()
	read, gotCred, ok := s.authorize(w, r, permJobsSubmit)
	if !ok {
		t.Fatalf("authorize: %d %s", w.Code, w.Body.String())
	}
	if string(read) != body {
		t.Errorf("body mismatch: %q", read)
	}
	if gotCred.ID != "de1" {
		t.Errorf("cred = %+v", gotCred)
	}
}

func TestAuthorizeMissingService(t *testing.T) {
	s := newTestServer(t, "")
	w := httptest.NewRecorder()
	if _, _, ok := s.authorize(w, httptest.NewRequest("POST", "/v1/jobs", strings.NewReader("{}")), permJobsSubmit); ok {
		t.Fatal("missing service header must be rejected")
	}
	if w.Code != http.StatusUnauthorized {
		t.Errorf("code = %d", w.Code)
	}
}

func TestAuthorizeUnknownService(t *testing.T) {
	s := newTestServer(t, "")
	r := signedRequest(RealmCred{ServiceID: "other", Secret: "x"}, "POST", "/v1/jobs", "", time.Now().Unix(), "{}")
	w := httptest.NewRecorder()
	if _, _, ok := s.authorize(w, r, permJobsSubmit); ok {
		t.Fatal("unknown service must be rejected")
	}
	if w.Code != http.StatusUnauthorized {
		t.Errorf("code = %d", w.Code)
	}
}

func TestAuthorizeStaleTimestamp(t *testing.T) {
	s := newTestServer(t, "")
	cred := s.cfg.Realms["de1"]
	r := signedRequest(cred, "POST", "/v1/jobs", "", time.Now().Add(2*time.Hour).Unix(), "{}")
	w := httptest.NewRecorder()
	if _, _, ok := s.authorize(w, r, permJobsSubmit); ok {
		t.Fatal("stale timestamp must be rejected")
	}
	if w.Code != http.StatusUnauthorized {
		t.Errorf("code = %d", w.Code)
	}
}

func TestAuthorizeBadSignature(t *testing.T) {
	s := newTestServer(t, "")
	cred := s.cfg.Realms["de1"]
	r := signedRequest(cred, "POST", "/v1/jobs", "", time.Now().Unix(), `{"text":"a"}`)
	r.Header.Set("X-Andora-Signature", "deadbeef")
	w := httptest.NewRecorder()
	if _, _, ok := s.authorize(w, r, permJobsSubmit); ok {
		t.Fatal("bad signature must be rejected")
	}
	if w.Code != http.StatusUnauthorized {
		t.Errorf("code = %d", w.Code)
	}
}

func TestAuthorizeMissingPermission(t *testing.T) {
	s := newTestServer(t, "SERVICE_DE1_PERMISSIONS=coordinator.jobs.query\n")
	cred := s.cfg.Realms["de1"]
	r := signedRequest(cred, "POST", "/v1/jobs", "", time.Now().Unix(), "{}")
	w := httptest.NewRecorder()
	if _, _, ok := s.authorize(w, r, permJobsSubmit); ok {
		t.Fatal("missing submit permission must be rejected")
	}
	if w.Code != http.StatusForbidden {
		t.Errorf("code = %d, want 403", w.Code)
	}
}

func TestAuthorizeRateLimit(t *testing.T) {
	s := newTestServer(t, "RATE_LIMIT_BURST=2\nRATE_LIMIT_PER_MIN=100\n")
	cred := s.cfg.Realms["de1"]
	for i := 0; i < 3; i++ {
		r := signedRequest(cred, "POST", "/v1/jobs", "", time.Now().Unix(), "{}")
		w := httptest.NewRecorder()
		_, _, ok := s.authorize(w, r, permJobsSubmit)
		if i < 2 && !ok {
			t.Fatalf("request %d must pass", i)
		}
		if i == 2 {
			if ok {
				t.Fatal("third request must be rate-limited")
			}
			if w.Code != http.StatusTooManyRequests {
				t.Errorf("code = %d, want 429", w.Code)
			}
			if w.Header().Get("Retry-After") == "" {
				t.Error("missing Retry-After")
			}
		}
	}
}

func TestNoCacheHeaders(t *testing.T) {
	s := newTestServer(t, "")
	h := s.handler()
	r := httptest.NewRequest("GET", "/health", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	if cc := w.Header().Get("Cache-Control"); cc != "no-store, no-cache, must-revalidate" {
		t.Errorf("Cache-Control = %q", cc)
	}
	if pr := w.Header().Get("Pragma"); pr != "no-cache" {
		t.Errorf("Pragma = %q", pr)
	}
}

func TestDecodeJSON(t *testing.T) {
	type payload struct {
		JobType string `json:"job_type"`
		Text    string `json:"text"`
	}
	var p payload
	if !decodeJSON([]byte(`{"job_type":"player","text":"hi"}`), &p) {
		t.Fatal("valid JSON must decode")
	}
	if p.JobType != "player" || p.Text != "hi" {
		t.Errorf("payload = %+v", p)
	}
	if decodeJSON([]byte(`{oops`), &p) {
		t.Error("broken JSON must fail")
	}
}
