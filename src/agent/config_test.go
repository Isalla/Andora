package main

import (
	"os"
	"path/filepath"
	"testing"
	"time"
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

func loadEnvString(t *testing.T, content string) *Config {
	t.Helper()
	p := writeEnv(t, content)
	cfg, err := loadConfig(p)
	if err != nil {
		t.Fatalf("loadConfig: %v", err)
	}
	return cfg
}

const minimalEnv = "AGENT_TOKEN=test-token\n" +
	"AGENT_SERVICES=realm\n" +
	"AGENT_SERVICE_REALM_UNIT=andora-server.service\n"

func TestParseEnvFile(t *testing.T) {
	content := "# comment\nPORT=8080\n# another comment\nHOST=127.0.0.1\nEMPTY=\nSPACES= hello \n"
	vals, err := parseEnvFile(writeEnv(t, content))
	if err != nil {
		t.Fatal(err)
	}
	if vals["PORT"] != "8080" {
		t.Errorf("PORT = %q, want 8080", vals["PORT"])
	}
	if vals["HOST"] != "127.0.0.1" {
		t.Errorf("HOST = %q", vals["HOST"])
	}
	if vals["SPACES"] != "hello" {
		t.Errorf("SPACES = %q, want trimmed", vals["SPACES"])
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
	if cfg.Port != 9443 {
		t.Errorf("Port = %d, want 9443", cfg.Port)
	}
	if cfg.Token != "test-token" {
		t.Errorf("Token = %q", cfg.Token)
	}
	if cfg.UseSudo != true {
		t.Errorf("UseSudo = %v, want true", cfg.UseSudo)
	}
	if cfg.LogLineLimit != 200 {
		t.Errorf("LogLineLimit = %d", cfg.LogLineLimit)
	}
	if cfg.ProbeTimeout != 1500*time.Millisecond {
		t.Errorf("ProbeTimeout = %v", cfg.ProbeTimeout)
	}
	if len(cfg.Services) != 1 {
		t.Fatalf("Services = %d, want 1", len(cfg.Services))
	}
	svc := cfg.Services[0]
	if svc.Key != "realm" || svc.Unit != "andora-server.service" {
		t.Errorf("service = %+v", svc)
	}
	if svc.HealthPath != "/health" {
		t.Errorf("HealthPath = %q", svc.HealthPath)
	}
}

func TestLoadConfigOverrides(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv+
		"AGENT_PORT=8888\n"+
		"AGENT_SUDO_ENABLED=false\n"+
		"AGENT_LOG_LINE_LIMIT=50\n"+
		"AGENT_SERVICE_REALM_HEALTH_PATH=/status\n")
	if cfg.Port != 8888 {
		t.Errorf("Port = %d", cfg.Port)
	}
	if cfg.UseSudo != false {
		t.Errorf("UseSudo = %v", cfg.UseSudo)
	}
	if cfg.LogLineLimit != 50 {
		t.Errorf("LogLineLimit = %d", cfg.LogLineLimit)
	}
	if cfg.Services[0].HealthPath != "/status" {
		t.Errorf("HealthPath = %q", cfg.Services[0].HealthPath)
	}
}

func TestLoadConfigRequiresToken(t *testing.T) {
	_, err := loadConfig(writeEnv(t, "AGENT_SERVICES=realm\nAGENT_SERVICE_REALM_UNIT=andora-server.service\n"))
	if err == nil {
		t.Error("missing token must fail")
	}
}

func TestLoadConfigRequiresServices(t *testing.T) {
	_, err := loadConfig(writeEnv(t, "AGENT_TOKEN=tok\n"))
	if err == nil {
		t.Error("missing services must fail")
	}
}

func TestLoadConfigRejectsDuplicateKeys(t *testing.T) {
	_, err := loadConfig(writeEnv(t, minimalEnv+"AGENT_SERVICES=realm,realm\n"))
	if err == nil {
		t.Error("duplicate key must fail")
	}
}

func TestLoadConfigRejectsDuplicateUnits(t *testing.T) {
	_, err := loadConfig(writeEnv(t, minimalEnv+
		"AGENT_SERVICES=realm,server\n"+
		"AGENT_SERVICE_SERVER_UNIT=andora-server.service\n"))
	if err == nil {
		t.Error("duplicate unit must fail")
	}
}

func TestLoadConfigRejectsInvalidUnit(t *testing.T) {
	_, err := loadConfig(writeEnv(t, "AGENT_TOKEN=tok\nAGENT_SERVICES=x\nAGENT_SERVICE_X_UNIT=bad; rm -rf /\n"))
	if err == nil {
		t.Error("invalid unit must fail")
	}
}

func TestLoadConfigRejectsInvalidKey(t *testing.T) {
	_, err := loadConfig(writeEnv(t, "AGENT_TOKEN=tok\nAGENT_SERVICES=bad key\n"))
	if err == nil {
		t.Error("invalid key must fail")
	}
}

func TestLoadConfigRequiresUnitPerService(t *testing.T) {
	_, err := loadConfig(writeEnv(t, "AGENT_TOKEN=tok\nAGENT_SERVICES=realm\n"))
	if err == nil {
		t.Error("missing unit must fail")
	}
}

func TestLoadConfigRejectsPartialMTLS(t *testing.T) {
	_, err := loadConfig(writeEnv(t, minimalEnv+"AGENT_CA_FILE=/tmp/ca.pem\n"))
	if err == nil {
		t.Error("partial mTLS must fail")
	}
}

func TestLoadConfigRejectsMTLSWithoutTLS(t *testing.T) {
	_, err := loadConfig(writeEnv(t, minimalEnv+
		"AGENT_CA_FILE=/tmp/ca.pem\n"+
		"AGENT_CLIENT_CERT_FILE=/tmp/c.pem\n"+
		"AGENT_CLIENT_KEY_FILE=/tmp/k.pem\n"))
	if err == nil {
		t.Error("mTLS without server TLS must fail (no silent degradation)")
	}
}

func TestLoadConfigAcceptsTLSAndMTLS(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv+
		"TLS_CERT_FILE=/tmp/cert.pem\n"+
		"TLS_KEY_FILE=/tmp/key.pem\n"+
		"AGENT_CA_FILE=/tmp/ca.pem\n"+
		"AGENT_CLIENT_CERT_FILE=/tmp/c.pem\n"+
		"AGENT_CLIENT_KEY_FILE=/tmp/k.pem\n")
	if cfg.TLSCertFile != "/tmp/cert.pem" || cfg.ClientCAFile != "/tmp/ca.pem" {
		t.Errorf("TLS/mTLS config not loaded: %+v", cfg)
	}
}

func TestLoadConfigRejectsPartialTLS(t *testing.T) {
	_, err := loadConfig(writeEnv(t, minimalEnv+"TLS_CERT_FILE=/tmp/cert.pem\n"))
	if err == nil {
		t.Error("partial TLS must fail")
	}
}

func TestLoadConfigRejectsUnknownBindHost(t *testing.T) {
	_, err := loadConfig(writeEnv(t, minimalEnv+"AGENT_BIND_HOST=nonexistent.example\n"))
	if err == nil {
		t.Error("unknown bind host must fail")
	}
}

func TestConfigPath(t *testing.T) {
	prev := os.Getenv("AGENT_CONFIG")
	defer os.Setenv("AGENT_CONFIG", prev)

	os.Setenv("AGENT_CONFIG", "/env/path.env")
	if got, _ := configPath([]string{"agent"}); got != "/env/path.env" {
		t.Errorf("env config = %q", got)
	}
	if got, _ := configPath([]string{"agent", "b.env"}); got != "b.env" {
		t.Errorf("cli arg must win over env, got %q", got)
	}
	os.Unsetenv("AGENT_CONFIG")
	if got, _ := configPath([]string{"agent"}); got != "config.env" {
		t.Errorf("default = %q", got)
	}
}
