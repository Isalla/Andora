package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

// Config is loaded from an EnvironmentFile-style file. Optional
// argument:
//
//	./login [/path/to/config.env]
//
// The path may also be given via LOGIN_CONFIG. If neither is given,
// "config.env" next to the binary is used.
//
// The login service holds NO database credentials. It talks to the
// Auth/API-Service with its OWN service credential (login-service) and
// only the permissions the login flow actually needs (see
// docs/Auth_API_Architektur.md section 10, LOGIN set).
type Config struct {
	Port int
	// AuthAPI is the base URL of the Auth/API-Service, e.g.
	// http://127.0.0.1:8080 (no trailing slash).
	AuthAPIURL string
	ServiceID  string
	Secret     string
	// RealmWS maps realm_id -> websocket URL of that realm's game
	// server (operator deployment config, e.g. "1=ws://10.0.0.5:3001").
	// Parsed from REALM_WS_URLS ("id=url,id=url"). Realms without an
	// entry get an empty ws_url in handoff answers.
	RealmWS map[int]string
	// AuthAPITimeout bounds a single Auth-API call.
	AuthAPITimeout time.Duration
	RateLimitBurst  int
	RateLimitPerMin int
	// Optional TLS (certificate + key path; empty disables).
	TLSCertFile string
	TLSKeyFile  string
}

func intEnv(env map[string]string, key string, def int) int {
	v := env[key]
	if v == "" {
		return def
	}
	n, err := strconv.Atoi(v)
	if err != nil {
		return def
	}
	return n
}

// parseEnvFile reads KEY=VALUE lines; '#' comments and leading
// whitespace are allowed. A missing file is tolerated (empty env).
func parseEnvFile(path string) (map[string]string, error) {
	out := map[string]string{}
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return out, nil
		}
		return nil, err
	}
	for _, raw := range strings.Split(string(data), "\n") {
		line := strings.TrimSpace(raw)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		ie := strings.Index(line, "=")
		if ie <= 0 {
			continue
		}
		key := strings.TrimSpace(line[:ie])
		val := strings.TrimSpace(line[ie+1:])
		out[key] = val
	}
	return out, nil
}

// parseRealmWS parses "id=url,id=url" pairs (whitespace tolerated).
// Malformed pairs abort the service: an unclear realm mapping must
// never silently drop a realm from handoff answers.
func parseRealmWS(s string) (map[int]string, error) {
	out := map[int]string{}
	if strings.TrimSpace(s) == "" {
		return out, nil
	}
	for _, pair := range strings.Split(s, ",") {
		pair = strings.TrimSpace(pair)
		if pair == "" {
			continue
		}
		ie := strings.Index(pair, "=")
		if ie <= 0 {
			return nil, fmt.Errorf("bad REALM_WS_URLS pair %q (want id=url)", pair)
		}
		id, err := strconv.Atoi(strings.TrimSpace(pair[:ie]))
		if err != nil || id <= 0 {
			return nil, fmt.Errorf("bad REALM_WS_URLS realm id in %q", pair)
		}
		url := strings.TrimSpace(pair[ie+1:])
		if url == "" {
			return nil, fmt.Errorf("bad REALM_WS_URLS empty url in %q", pair)
		}
		if _, dup := out[id]; dup {
			return nil, fmt.Errorf("duplicate REALM_WS_URLS realm id %d", id)
		}
		out[id] = url
	}
	return out, nil
}

// loadConfig validates and builds the full configuration. Missing
// required values abort the service with a clear error.
func loadConfig(path string) (*Config, error) {
	env, err := parseEnvFile(path)
	if err != nil {
		return nil, fmt.Errorf("read config %s: %w", path, err)
	}
	get := func(k string) string { return env[k] }

	url := strings.TrimRight(get("AUTHAPI_URL"), "/")
	if url == "" {
		return nil, fmt.Errorf("AUTHAPI_URL is required")
	}
	sid, secret := get("LOGIN_SERVICE_ID"), get("LOGIN_SERVICE_SECRET")
	if sid == "" || secret == "" {
		return nil, fmt.Errorf("LOGIN_SERVICE_ID and LOGIN_SERVICE_SECRET are required")
	}
	realmWS, err := parseRealmWS(get("REALM_WS_URLS"))
	if err != nil {
		return nil, err
	}
	return &Config{
		Port:           intEnv(env, "LOGIN_PORT", 8081),
		AuthAPIURL:     url,
		ServiceID:      sid,
		Secret:         secret,
		RealmWS:        realmWS,
		AuthAPITimeout: time.Duration(intEnv(env, "AUTHAPI_TIMEOUT_MS", 5000)) * time.Millisecond,
		RateLimitBurst: intEnv(env, "RATE_LIMIT_BURST", 10),
		RateLimitPerMin: intEnv(env, "RATE_LIMIT_PER_MIN", 120),
		TLSCertFile:    get("TLS_CERT_FILE"),
		TLSKeyFile:     get("TLS_KEY_FILE"),
	}, nil
}

// configPath resolves the location of the config file.
func configPath(args []string) (string, error) {
	if len(args) > 1 {
		return args[1], nil
	}
	if p := os.Getenv("LOGIN_CONFIG"); p != "" {
		return p, nil
	}
	return "config.env", nil
}
