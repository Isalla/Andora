package main

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func writeEnv(t *testing.T, content string) string {
	t.Helper()
	dir := t.TempDir()
	p := filepath.Join(dir, "config.env")
	if err := os.WriteFile(p, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	return p
}

// loadEnvString loads a configuration from an in-memory env file. The
// caller provides a complete minimal configuration.
func loadEnvString(t *testing.T, content string) *Config {
	t.Helper()
	p := writeEnv(t, content)
	cfg, err := loadConfig(p)
	if err != nil {
		t.Fatalf("loadConfig: %v", err)
	}
	return cfg
}

const minimalEnv = "OLLAMA_URL=http://127.0.0.1:11434\n" +
	"OLLAMA_MODEL=test-model\n" +
	"SERVICE_DE1_ID=realm-de1-service\n" +
	"SERVICE_DE1_SECRET=s1\n" +
	"SERVICE_DE1_PERMISSIONS=coordinator.jobs.submit, coordinator.jobs.query\n" +
	"REALM_DE1_ID=de1\n"

func TestParseEnvFile(t *testing.T) {
	vals, err := parseEnvFile("fixtures/env_basic.env")
	if err != nil {
		t.Fatal(err)
	}
	if got := vals["APP_PORT"]; got != "3030" {
		t.Errorf("APP_PORT = %q, want 3030", got)
	}
	if got := vals["STREET_STUFF"]; got != "allow # literal" {
		t.Errorf("STREET_STUFF must keep # literally, got %q", got)
	}
	if _, ok := vals["COMMENT_ONLY"]; ok {
		t.Errorf("COMMENT_ONLY must stay unset")
	}
	if got := vals["SPACES"]; got != "with spaces" {
		t.Errorf("SPACES = %q", got)
	}
	if got := vals["SECRET"]; !strings.HasPrefix(got, "=pwd=") {
		t.Errorf("SECRET = %q, want to keep leading =", got)
	}
	if got := vals["EMPTY"]; got != "" {
		t.Errorf("EMPTY = %q", got)
	}
	if _, ok := vals["BROKEN_LINE"]; ok {
		t.Errorf("BROKEN_LINE must stay unset")
	}
}

func TestParseEnvFileMissingFileIsTolerated(t *testing.T) {
	vals, err := parseEnvFile(filepath.Join(t.TempDir(), "nope.env"))
	if err != nil {
		t.Fatal(err)
	}
	if len(vals) != 0 {
		t.Errorf("expected empty env, got %v", vals)
	}
}

func TestLoadConfigDefaults(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv)
	if cfg.Port != 8082 || cfg.Workers != 2 || cfg.QueueMaxSize != 200 {
		t.Errorf("unexpected defaults: %+v", cfg)
	}
	if cfg.NumCtx != 8192 || cfg.ContextReservedTokens != 1536 {
		t.Errorf("context defaults wrong: NumCtx=%d reserved=%d", cfg.NumCtx, cfg.ContextReservedTokens)
	}
	if cfg.OllamaTimeout.String() != "15s" {
		t.Errorf("OllamaTimeout = %v", cfg.OllamaTimeout)
	}
	if cfg.MaxAttempts != 5 || cfg.MaxTextChars != 500 {
		t.Errorf("attempts/chars defaults wrong: %d/%d", cfg.MaxAttempts, cfg.MaxTextChars)
	}
	if cfg.OllamaURL != "http://127.0.0.1:11434" {
		t.Errorf("OllamaURL = %q", cfg.OllamaURL)
	}
	if len(cfg.Realms) != 1 {
		t.Fatalf("realms = %d", len(cfg.Realms))
	}
	cred := cfg.Realms["de1"]
	if cred.ServiceID != "realm-de1-service" || cred.Secret != "s1" {
		t.Errorf("cred = %+v", cred)
	}
	if !cred.hasPermission("coordinator.jobs.submit") || !cred.hasPermission("coordinator.jobs.query") {
		t.Errorf("permissions = %v", cred.Permissions)
	}
	if cred.hasPermission("coordinator.jobs.delete") {
		t.Error("closed permission set must reject unknown permissions")
	}
}

func TestLoadConfigOverrides(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv+
		"COORDINATOR_PORT=9999\n"+
		"COORDINATOR_WORKERS=4\n"+
		"OLLAMA_URL=http://[::1]:11434\n"+
		"OLLAMA_NUM_CTX=4096\n"+
		"PRIORITY_WORLD=5\n")
	if cfg.Port != 9999 || cfg.Workers != 4 {
		t.Errorf("overrides not applied: %+v", cfg)
	}
	if cfg.OllamaURL != "http://[::1]:11434" {
		t.Errorf("OllamaURL = %q", cfg.OllamaURL)
	}
	if cfg.NumCtx != 4096 {
		t.Errorf("NumCtx = %d", cfg.NumCtx)
	}
	if cfg.Priorities["world"] != 5 {
		t.Errorf("world priority = %d", cfg.Priorities["world"])
	}
	if cfg.Priorities["raidboss"] != 100 {
		t.Errorf("raidboss priority default = %d", cfg.Priorities["raidboss"])
	}
}

func TestLoadConfigMalformedURL(t *testing.T) {
	cases := []string{
		"OLLAMA_URL=://bad\n",  // no scheme
		"OLLAMA_URL=notaurl\n", // no "://"
	}
	for _, c := range cases {
		if _, err := loadConfig(writeEnv(t, c)); err == nil {
			t.Errorf("loadConfig(%q) must fail", c)
		}
	}
}

func TestLoadConfigRequiredFields(t *testing.T) {
	cases := []string{
		"OLLAMA_URL=http://127.0.0.1:11434\n" + // no model
			"SERVICE_DE1_ID=r\nSERVICE_DE1_SECRET=s\nSERVICE_DE1_PERMISSIONS=coordinator.jobs.submit\nREALM_DE1_ID=de1\n",
		"OLLAMA_MODEL=m\n" + // no url
			"SERVICE_DE1_ID=r\nSERVICE_DE1_SECRET=s\nSERVICE_DE1_PERMISSIONS=coordinator.jobs.submit\nREALM_DE1_ID=de1\n",
		"OLLAMA_URL=http://127.0.0.1:11434\nOLLAMA_MODEL=m\n", // no realms
	}
	for _, c := range cases {
		if _, err := loadConfig(writeEnv(t, c)); err == nil {
			t.Errorf("loadConfig must fail for incomplete config:\n%s", c)
		}
	}
}

func TestLoadConfigRejectsUnknownBindHost(t *testing.T) {
	if _, err := loadConfig(writeEnv(t, minimalEnv+"COORDINATOR_BIND_HOST=nonexistent.example\n")); err == nil {
		t.Error("unknown bind host must fail")
	}
}

func TestParseRealms(t *testing.T) {
	in := map[string]string{
		"SERVICE_DE1_ID":          "realm-de1-service",
		"SERVICE_DE1_SECRET":      "s1",
		"SERVICE_DE1_PERMISSIONS": "coordinator.jobs.submit, coordinator.jobs.query",
		"REALM_DE1_ID":            "de1",
		"REALM_DE1_RESULT_URL":    "https://realm-de1.example.invalid/v1/coordinator/result",
	}
	got, err := parseRealms(in)
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]string{"de1": "realm-de1-service"}
	if !reflect.DeepEqual(keys(got), keys(want)) {
		t.Fatalf("parseRealms keys = %v, want %v", keys(got), keys(want))
	}
	cred := got["de1"]
	if cred.ID != "de1" || cred.ServiceID != "realm-de1-service" || cred.Secret != "s1" {
		t.Errorf("cred = %+v", cred)
	}
	if len(cred.Permissions) != 2 || !cred.hasPermission("coordinator.jobs.submit") {
		t.Errorf("permissions = %v", cred.Permissions)
	}
	if cred.ResultURL != "https://realm-de1.example.invalid/v1/coordinator/result" {
		t.Errorf("ResultURL = %q", cred.ResultURL)
	}
}

func TestParseRealmsIPv6ResultURL(t *testing.T) {
	cred, err := parseRealms(map[string]string{
		"SERVICE_DE1_ID":          "r",
		"SERVICE_DE1_SECRET":      "s",
		"SERVICE_DE1_PERMISSIONS": "coordinator.jobs.submit",
		"REALM_DE1_ID":            "de1",
		"REALM_DE1_RESULT_URL":    "http://2001:db8::1/v1/coordinator/result",
	})
	if err != nil {
		t.Fatal(err)
	}
	if got := cred["de1"].ResultURL; got != "http://[2001:db8::1]/v1/coordinator/result" {
		t.Errorf("ResultURL = %q", got)
	}
}

func TestParseRealmsIncompleteFails(t *testing.T) {
	cases := []map[string]string{
		{ // missing REALM mapping
			"SERVICE_DE1_ID": "r", "SERVICE_DE1_SECRET": "s",
			"SERVICE_DE1_PERMISSIONS": "coordinator.jobs.submit",
		},
		{ // missing secret
			"SERVICE_DE1_ID":          "r",
			"SERVICE_DE1_PERMISSIONS": "coordinator.jobs.submit",
			"REALM_DE1_ID":            "de1",
		},
	}
	for _, in := range cases {
		if _, err := parseRealms(in); err == nil {
			t.Errorf("parseRealms(%v) must fail", in)
		}
	}
}

func TestParseRealmsDuplicateRealmIDFails(t *testing.T) {
	_, err := parseRealms(map[string]string{
		"SERVICE_DE1_ID":          "r1",
		"SERVICE_DE1_SECRET":      "s1",
		"SERVICE_DE1_PERMISSIONS": "coordinator.jobs.submit",
		"REALM_DE1_ID":            "de1",
		"SERVICE_DE2_ID":          "r2",
		"SERVICE_DE2_SECRET":      "s2",
		"SERVICE_DE2_PERMISSIONS": "coordinator.jobs.submit",
		"REALM_DE2_ID":            "de1", // duplicate public id
	})
	if err == nil {
		t.Error("duplicate realm public id must fail")
	}
}

func TestSplitList(t *testing.T) {
	in := "a,,b,  c ;d"
	got := splitList(in)
	want := []string{"a", "b", "c", "d"}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("splitList = %v, want %v", got, want)
	}
}

func TestConfigPath(t *testing.T) {
	prev := os.Getenv("COORDINATOR_CONFIG")
	defer os.Setenv("COORDINATOR_CONFIG", prev)

	os.Setenv("COORDINATOR_CONFIG", "/env/path.env")
	if got, _ := configPath([]string{"coordinator"}); got != "/env/path.env" {
		t.Errorf("env config = %q", got)
	}
	if got, _ := configPath([]string{"coordinator", "b.env"}); got != "b.env" {
		t.Errorf("cli arg must win over env, got %q", got)
	}
	os.Unsetenv("COORDINATOR_CONFIG")
	if got, _ := configPath([]string{"coordinator"}); got != "config.env" {
		t.Errorf("default = %q", got)
	}
}

func keys[V any](m map[string]V) []string {
	var out []string
	for k := range m {
		out = append(out, k)
	}
	return out
}
