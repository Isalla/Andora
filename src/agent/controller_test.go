package main

import (
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"
)

// fakeRunner records args and returns canned output.
type fakeRunner struct {
	results map[string]string // full command string -> stdout
	calls   [][]string
}

func (f *fakeRunner) Run(args []string) (string, string, int) {
	f.calls = append(f.calls, args)
	joined := strings.Join(args, " ")
	if out, ok := f.results[joined]; ok {
		return out, "", 0
	}
	return "", "fake: no result for " + joined, 1
}

func testController(t *testing.T, envExtra string, runner Runner) (*Config, *Controller) {
	t.Helper()
	env := minimalEnv + envExtra
	if envExtra == "" {
		env = minimalEnv
	}
	cfg := loadEnvString(t, env)
	ctl := NewController(cfg, runner)
	return cfg, ctl
}

func TestControllerStateParsesActive(t *testing.T) {
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/systemctl show --property=ActiveState --value --no-pager andora-server.service": "active",
	}}
	_, ctl := testController(t, "", fr)
	if got := ctl.State("andora-server.service"); got != "active" {
		t.Errorf("state = %q, want active", got)
	}
}

func TestControllerStateUnknownOnFailure(t *testing.T) {
	fr := &fakeRunner{} // no results -> exit 1
	_, ctl := testController(t, "", fr)
	if got := ctl.State("andora-server.service"); got != "unknown" {
		t.Errorf("state = %q, want unknown", got)
	}
}

func TestControllerActionUsesSudo(t *testing.T) {
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/sudo -n /usr/bin/systemctl start andora-server.service": "job 1 running",
	}}
	_, ctl := testController(t, "", fr)
	out, err := ctl.Action("start", "andora-server.service")
	if err != nil {
		t.Fatal(err)
	}
	if out != "job 1 running" {
		t.Errorf("out = %q", out)
	}
	want := []string{"/usr/bin/sudo", "-n", "/usr/bin/systemctl", "start", "andora-server.service"}
	if !reflect.DeepEqual(fr.calls[0], want) {
		t.Errorf("calls = %v, want %v", fr.calls[0], want)
	}
}

func TestControllerActionNoSudo(t *testing.T) {
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/systemctl restart andora-server.service": "",
	}}
	cfg := loadEnvString(t, minimalEnv+"AGENT_SUDO_ENABLED=false\n")
	ctl := NewController(cfg, fr)
	if _, err := ctl.Action("restart", "andora-server.service"); err != nil {
		t.Fatal(err)
	}
	want := []string{"/usr/bin/systemctl", "restart", "andora-server.service"}
	if !reflect.DeepEqual(fr.calls[0], want) {
		t.Errorf("calls = %v, want %v", fr.calls[0], want)
	}
}

func TestControllerActionRejectsInvalidVerb(t *testing.T) {
	_, ctl := testController(t, "", &fakeRunner{})
	if _, err := ctl.Action("hack", "andora-server.service"); err == nil {
		t.Error("invalid verb must fail")
	}
}

func TestControllerActionRejectsMissingVerb(t *testing.T) {
	_, ctl := testController(t, "", &fakeRunner{})
	if _, err := ctl.Action("", "andora-server.service"); err == nil {
		t.Error("empty verb must fail")
	}
}

func TestControllerLogsCapsLines(t *testing.T) {
	longLog := ""
	for i := 0; i < 50; i++ {
		longLog += "line " + string(rune('a'+i%26)) + "\n"
	}
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/sudo -n /usr/bin/journalctl --no-pager --lines 10 -u andora-server.service": longLog,
	}}
	cfg := loadEnvString(t, minimalEnv+"AGENT_LOG_LINE_LIMIT=10\n")
	ctl := NewController(cfg, fr)
	lines, _ := ctl.Logs("andora-server.service", 100)
	if len(lines) != 10 {
		t.Errorf("lines = %d, want 10", len(lines))
	}
	want := []string{"line o", "line p", "line q", "line r", "line s", "line t", "line u", "line v", "line w", "line x"}
	if !reflect.DeepEqual(lines, want) {
		t.Errorf("tail mismatch: %v", lines)
	}
}

func TestControllerLogsCapsBytes(t *testing.T) {
	longLog := strings.Repeat("x", 3000) + "\n"
	fr := &fakeRunner{results: map[string]string{
		"/usr/bin/sudo -n /usr/bin/journalctl --no-pager --lines 200 -u andora-server.service": longLog,
	}}
	cfg := loadEnvString(t, minimalEnv+"AGENT_LOG_BYTE_LIMIT=2048\n")
	ctl := NewController(cfg, fr)
	lines, truncated := ctl.Logs("andora-server.service", 200)
	if !truncated {
		t.Error("must mark truncated")
	}
	// 2048 bytes of x, no trailing newline -> one line.
	if len(lines) != 1 || len(lines[0]) != 2048 {
		t.Errorf("lines=%v len=%d", lines, len(lines))
	}
}

func TestControllerLogsUnitUnknownReturnsEmpty(t *testing.T) {
	fr := &fakeRunner{} // fails
	_, ctl := testController(t, "", fr)
	lines, _ := ctl.Logs("andora-server.service", 10)
	if len(lines) != 0 {
		t.Errorf("lines = %v, want empty on failure", lines)
	}
}

func TestControllerHealthyAcceptGoStyle(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"status":"ok"}`))
	}))
	defer srv.Close()
	cfg := loadEnvString(t, minimalEnv+"AGENT_SERVICE_REALM_URL="+srv.URL+"\n")
	ctl := NewController(cfg, nil)
	svc := cfg.Services[0]
	healthy, detail := ctl.Healthy(svc)
	if !healthy || detail != "ok" {
		t.Errorf("healthy=%v detail=%q", healthy, detail)
	}
}

func TestControllerHealthyAcceptRealmStyle(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"ok":true,"realm_id":"de1"}`))
	}))
	defer srv.Close()
	cfg := loadEnvString(t, minimalEnv+"AGENT_SERVICE_REALM_URL="+srv.URL+"\n")
	ctl := NewController(cfg, nil)
	svc := cfg.Services[0]
	healthy, _ := ctl.Healthy(svc)
	if !healthy {
		t.Errorf("healthy = %v, want true", healthy)
	}
}

func TestControllerHealthyRejectsNon200(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer srv.Close()
	cfg := loadEnvString(t, minimalEnv+"AGENT_SERVICE_REALM_URL="+srv.URL+"\n")
	ctl := NewController(cfg, nil)
	svc := cfg.Services[0]
	healthy, detail := ctl.Healthy(svc)
	if healthy || !strings.Contains(detail, "500") {
		t.Errorf("healthy=%v detail=%q", healthy, detail)
	}
}

func TestControllerHealthyNoURLConfigured(t *testing.T) {
	cfg, ctl := testController(t, "", &fakeRunner{})
	svc := cfg.Services[0]
	healthy, detail := ctl.Healthy(svc)
	if healthy {
		t.Errorf("healthy must be false without URL, detail=%q", detail)
	}
}

func TestControllerVersionFromStatus(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"status":"ok","version":"1.2.3"}`))
	}))
	defer srv.Close()
	cfg := loadEnvString(t, minimalEnv+"AGENT_SERVICE_REALM_URL="+srv.URL+"\n")
	ctl := NewController(cfg, nil)
	svc := cfg.Services[0]
	if got := ctl.Version(svc); got != "1.2.3" {
		t.Errorf("version = %q", got)
	}
}

func TestControllerVersionUnknown(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"status":"ok"}`))
	}))
	defer srv.Close()
	cfg := loadEnvString(t, minimalEnv+"AGENT_SERVICE_REALM_URL="+srv.URL+"\n")
	ctl := NewController(cfg, nil)
	svc := cfg.Services[0]
	if got := ctl.Version(svc); got != "unknown" {
		t.Errorf("version = %q, want unknown", got)
	}
}
