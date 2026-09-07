package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// testServer builds a full Server with a fake runner and returns it.
func testServer(t *testing.T, envExtra string, fr Runner) *Server {
	t.Helper()
	env := minimalEnv
	if envExtra != "" {
		env += envExtra
	}
	cfg := loadEnvString(t, env)
	ctl := NewController(cfg, fr)
	return newServer(cfg, ctl)
}

func get(t *testing.T, srv *Server, path, token string) (int, map[string]any) {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	if token != "" {
		req.Header.Set("X-Andora-Token", token)
	}
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	var m map[string]any
	_ = json.Unmarshal(rec.Body.Bytes(), &m)
	return rec.Code, m
}

func testAction(t *testing.T, srv *Server, path, token string) (int, map[string]any) {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(""))
	if token != "" {
		req.Header.Set("X-Andora-Token", token)
	}
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	var m map[string]any
	_ = json.Unmarshal(rec.Body.Bytes(), &m)
	return rec.Code, m
}

func TestHealthOpen(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	code, m := get(t, srv, "/health", "")
	if code != http.StatusOK || m["status"] != "ok" {
		t.Errorf("code=%d m=%v", code, m)
	}
}

func TestStatusOpen(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	code, m := get(t, srv, "/status", "")
	if code != http.StatusOK || m["status"] != "ok" {
		t.Errorf("code=%d m=%v", code, m)
	}
}

func TestManagementRequiresToken(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	code, _ := get(t, srv, "/api/v1/services", "")
	if code != http.StatusUnauthorized {
		t.Errorf("no token: code=%d, want 401", code)
	}
	code, _ = get(t, srv, "/api/v1/services", "wrong-token")
	if code != http.StatusUnauthorized {
		t.Errorf("bad token: code=%d, want 401", code)
	}
}

func TestManagementQueryToken(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	req := httptest.NewRequest(http.MethodGet, "/api/v1/services?token=test-token", nil)
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Errorf("query token: code=%d", rec.Code)
	}
}

func TestServicesList(t *testing.T) {
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/systemctl show --property=ActiveState --value --no-pager andora-server.service": "active",
	}}
	srv := testServer(t, "", fr)
	code, m := get(t, srv, "/api/v1/services", "test-token")
	if code != http.StatusOK {
		t.Fatalf("code=%d", code)
	}
	svcs, ok := m["services"].([]any)
	if !ok || len(svcs) != 1 {
		t.Fatalf("services = %v", m["services"])
	}
	first := svcs[0].(map[string]any)
	if first["key"] != "realm" || first["unit"] != "andora-server.service" {
		t.Errorf("first = %v", first)
	}
	if first["active"] != true {
		t.Errorf("active = %v", first["active"])
	}
}

func TestServiceUnknownKeyNotFound(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	code, _ := get(t, srv, "/api/v1/services/nope", "test-token")
	if code != http.StatusNotFound {
		t.Errorf("code=%d, want 404", code)
	}
}

func TestServiceDetail(t *testing.T) {
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/systemctl show --property=ActiveState --value --no-pager andora-server.service": "inactive",
	}}
	srv := testServer(t, "", fr)
	code, m := get(t, srv, "/api/v1/services/realm", "test-token")
	if code != http.StatusOK {
		t.Fatalf("code=%d", code)
	}
	if m["state"] != "inactive" || m["active"] != false {
		t.Errorf("m = %v", m)
	}
	if m["version"] != "unknown" {
		t.Errorf("version = %v", m["version"])
	}
}

func TestServiceActionStart(t *testing.T) {
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/sudo -n /usr/bin/systemctl start andora-server.service":                         "",
		"/usr/bin/systemctl show --property=ActiveState --value --no-pager andora-server.service": "active",
	}}
	srv := testServer(t, "", fr)
	code, m := testAction(t, srv, "/api/v1/services/realm/start", "test-token")
	if code != http.StatusOK {
		t.Fatalf("code=%d m=%v", code, m)
	}
	if m["action"] != "start" || m["state"] != "active" {
		t.Errorf("m = %v", m)
	}
}

func TestServiceActionFailsClosed(t *testing.T) {
	fr := &fakeRunner{} // no results -> exit 1
	srv := testServer(t, "", fr)
	code, _ := testAction(t, srv, "/api/v1/services/realm/stop", "test-token")
	if code != http.StatusBadGateway {
		t.Errorf("code=%d, want 502", code)
	}
}

func TestServiceActionGetNotAllowed(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	code, _ := get(t, srv, "/api/v1/services/realm/start", "test-token")
	if code != http.StatusMethodNotAllowed {
		t.Errorf("code=%d, want 405", code)
	}
}

func TestServiceLogsCapsRequested(t *testing.T) {
	logLine := strings.Repeat("x", 20) + "\n"
	var big strings.Builder
	for i := 0; i < 20; i++ {
		big.WriteString(logLine)
	}
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/sudo -n /usr/bin/journalctl --no-pager --lines 5 -u andora-server.service": big.String(),
	}}
	cfg := loadEnvString(t, minimalEnv+"AGENT_LOG_LINE_LIMIT=5\n")
	ctl := NewController(cfg, fr)
	srv := newServer(cfg, ctl)
	code, m := get(t, srv, "/api/v1/services/realm/logs?lines=1000", "test-token")
	if code != http.StatusOK {
		t.Fatalf("code=%d", code)
	}
	if m["returned"] != float64(5) {
		t.Errorf("returned = %v", m["returned"])
	}
	if m["requested"] != float64(5) {
		t.Errorf("requested = %v", m["requested"])
	}
}

func TestServiceLogsUnitUnknown(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	code, m := get(t, srv, "/api/v1/services/realm/logs", "test-token")
	if code != http.StatusOK {
		t.Fatalf("code=%d", code)
	}
	lines, ok := m["lines"].([]any)
	if !ok || len(lines) != 0 {
		t.Errorf("lines = %v", m["lines"])
	}
}

func TestServiceHealthEndpoint(t *testing.T) {
	health := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"ok":true}`))
	}))
	defer health.Close()
	srv := testServer(t, "AGENT_SERVICE_REALM_URL="+health.URL+"\n", &fakeRunner{})
	code, m := get(t, srv, "/api/v1/services/realm/health", "test-token")
	if code != http.StatusOK {
		t.Fatalf("code=%d", code)
	}
	if m["healthy"] != true {
		t.Errorf("healthy = %v", m["healthy"])
	}
}

func TestServiceVersionEndpoint(t *testing.T) {
	status := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"ok":true,"version":"1.2.3"}`))
	}))
	defer status.Close()
	srv := testServer(t, "AGENT_SERVICE_REALM_URL="+status.URL+"\n", &fakeRunner{})
	code, m := get(t, srv, "/api/v1/services/realm/version", "test-token")
	if code != http.StatusOK {
		t.Fatalf("code=%d", code)
	}
	if m["version"] != "1.2.3" {
		t.Errorf("version = %v", m["version"])
	}
}

func TestUnknownEndpoint(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	code, _ := get(t, srv, "/api/v1/services/realm/delete-all", "test-token")
	if code != http.StatusNotFound {
		t.Errorf("code=%d, want 404", code)
	}
}

func TestRateLimitApplies(t *testing.T) {
	srv := testServer(t, "", &fakeRunner{})
	srv.rl = newRateLimit(2, 100)
	// First two requests pass.
	for i := 0; i < 2; i++ {
		code, _ := get(t, srv, "/api/v1/services/realm", "test-token")
		if code != http.StatusOK {
			t.Errorf("request %d: code=%d", i, code)
		}
	}
	// Third is rate limited.
	code, _ := get(t, srv, "/api/v1/services/realm", "test-token")
	if code != http.StatusTooManyRequests {
		t.Errorf("rate limited: code=%d, want 429", code)
	}
}
