package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

// AuthDBConfig is the ONLY db configuration present in this service.
// Only the Auth/API-Service may hold AUTH_DB_* credentials. Consume
// services (web, login, realm) must not contain them.
type AuthDBConfig struct {
	Host     string
	Port     int
	User     string
	Password string
	Database string
}

// ServiceCred is the identity of ONE consume service. No shared
// secret: every consuming service has its own secret and only the
// API permissions that it actually needs.
type ServiceCred struct {
	ID          string
	Secret      string
	Permissions []string
}

// Defaults for the P-33 session monitoring. They bound the demand-driven
// collection on /status: at most one attempt per minimum interval, and at
// most defaultSessionStatsTimeoutMS per whole attempt.
const (
	defaultSessionStatsMinIntervalSecs = 60
	defaultSessionStatsTimeoutMS       = 5000
)

// Config is loaded from an EnvironmentFile-style file. Optional
// argument:
//
//	./authapi [/path/to/config.env]
//
// The path may also be given via AUTHAPI_CONFIG. If neither is given,
// "config.env" next to the binary is used.
type Config struct {
	Port     int
	BindHost string
	AuthDB   AuthDBConfig
	Services map[string]ServiceCred
	// TrustedProxies are the only sources whose X-Forwarded-For /
	// X-Real-IP headers are honored. Fail-closed: empty means the
	// forwarding headers are never believed.
	TrustedProxies *trustedProxySet
	ArgonTime      uint32
	ArgonMemory    uint32
	ArgonThreads   uint8
	// Security parameters.
	SessionTTL      time.Duration
	HandoffTTL      time.Duration
	RecoveryTTL     time.Duration
	CallbackURLHost string
	NewAccountBan   time.Duration
	// Rate limiting: burst allowed concurrently + requests per minute.
	RateLimitBurst  int
	RateLimitPerMin int
	// Session monitoring (P-33, read-only aggregates on /status).
	// SessionStatsEnabled=false suppresses the collection entirely and is
	// reported as such. The two limits bound one collection attempt:
	// SessionStatsMinIntervalSeconds is the minimum distance between two
	// attempts, SessionStatsTimeoutMS bounds a whole attempt. A value <= 0
	// falls back to the default (60 s / 5000 ms).
	SessionStatsEnabled         bool
	SessionStatsMinIntervalSecs int
	SessionStatsTimeoutMS       int
	// E-mail encryption key (32 bytes, hex) used for email_encrypted.
	// Loaded from config.env (outside git); never written to logs.
	EncryptionKey string
	// Optional TLS (certificate + key path; empty disables).
	TLSCertFile string
	TLSKeyFile  string
	// Migrations directory override (else <exe>/db/auth/migrations).
	MigrationsDir string
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

// positiveEnv behaves like intEnv but maps a non-positive or unparsable
// value back to the default, so a limit can never be configured away.
func positiveEnv(env map[string]string, key string, def int) int {
	n := intEnv(env, key, def)
	if n <= 0 {
		return def
	}
	return n
}

// boolEnv reads a boolean switch. Absent, empty or unparsable values keep
// the default; "1", "true", "yes" and "on" (any case) mean true.
func boolEnv(env map[string]string, key string, def bool) bool {
	v := strings.TrimSpace(env[key])
	if v == "" {
		return def
	}
	switch strings.ToLower(v) {
	case "1", "true", "yes", "on":
		return true
	case "0", "false", "no", "off":
		return false
	}
	return def
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

// loadConfig validates and builds the full configuration. Missing
// required values abort the service with a clear error.
func loadConfig(path string) (*Config, error) {
	env, err := parseEnvFile(path)
	if err != nil {
		return nil, fmt.Errorf("read config %s: %w", path, err)
	}
	get := func(k string) string { return env[k] }

	port := intEnv(env, "AUTHAPI_PORT", 8080)
	bindHost := strings.TrimSpace(get("AUTHAPI_BIND_HOST"))
	if _, err := listenHosts(bindHost, port); err != nil {
		return nil, fmt.Errorf("AUTHAPI_BIND_HOST: %w", err)
	}
	trusted, err := parseTrustedProxies(get("TRUSTED_PROXIES"))
	if err != nil {
		return nil, err
	}

	db := AuthDBConfig{
		Host:     get("AUTH_DB_HOST"),
		Port:     intEnv(env, "AUTH_DB_PORT", 3306),
		User:     get("AUTH_DB_USER"),
		Password: get("AUTH_DB_PASSWORD"),
		Database: get("AUTH_DB_NAME"),
	}
	if db.Database == "" {
		db.Database = "auth"
	}
	if db.Host == "" || db.User == "" || db.Password == "" {
		return nil, fmt.Errorf("AUTH_DB_HOST, AUTH_DB_USER, AUTH_DB_PASSWORD are required")
	}

	svc := make(map[string]ServiceCred)
	for k, v := range env {
		if strings.HasPrefix(k, "SERVICE_") && strings.HasSuffix(k, "_ID") {
			base := strings.TrimPrefix(k, "SERVICE_")
			base = strings.TrimSuffix(base, "_ID")
			sid := v
			secret := get("SERVICE_" + base + "_SECRET")
			perms := get("SERVICE_" + base + "_PERMISSIONS")
			if sid == "" || secret == "" || perms == "" {
				return nil, fmt.Errorf("service %q: _ID, _SECRET, _PERMISSIONS are all required", base)
			}
			// Keyed by the service ID: that is exactly the value a
			// consuming service presents in X-Andora-Service.
			svc[sid] = ServiceCred{
				ID:          sid,
				Secret:      secret,
				Permissions: splitPerms(perms),
			}
		}
	}
	if len(svc) == 0 {
		return nil, fmt.Errorf("at least one SERVICE_<name>_ID/_SECRET/_PERMISSIONS is required")
	}

	return &Config{
		Port:            port,
		BindHost:        bindHost,
		AuthDB:          db,
		Services:        svc,
		TrustedProxies:  trusted,
		ArgonTime:       uint32(intEnv(env, "ARGON_TIME", 1)),
		ArgonMemory:     uint32(intEnv(env, "ARGON_MEMORY", 19456)),
		ArgonThreads:    uint8(intEnv(env, "ARGON_THREADS", 4)),
		SessionTTL:      time.Duration(intEnv(env, "SESSION_TTL_MINUTES", 60)) * time.Minute,
		HandoffTTL:      time.Duration(intEnv(env, "HANDOFF_TTL_SECONDS", 60)) * time.Second,
		RecoveryTTL:     time.Duration(intEnv(env, "RECOVERY_TTL_MINUTES", 30)) * time.Minute,
		CallbackURLHost: get("RECOVERY_CALLBACK_HOST"),
		NewAccountBan:   time.Duration(intEnv(env, "NEW_ACCOUNT_BAN_SECONDS", 60)) * time.Second,
		RateLimitBurst:  intEnv(env, "RATE_LIMIT_BURST", 10),
		RateLimitPerMin: intEnv(env, "RATE_LIMIT_PER_MIN", 120),
		// Session monitoring (P-33). Names, defaults and units:
		//   SESSION_STATS_ENABLED            bool, default true
		//   SESSION_STATS_MIN_INTERVAL_SECS  int seconds, default 60 (<=0 -> 60)
		//   SESSION_STATS_TIMEOUT_MS         int milliseconds, default 5000 (<=0 -> 5000)
		SessionStatsEnabled:         boolEnv(env, "SESSION_STATS_ENABLED", true),
		SessionStatsMinIntervalSecs: positiveEnv(env, "SESSION_STATS_MIN_INTERVAL_SECS", defaultSessionStatsMinIntervalSecs),
		SessionStatsTimeoutMS:       positiveEnv(env, "SESSION_STATS_TIMEOUT_MS", defaultSessionStatsTimeoutMS),
		EncryptionKey:               get("ENCRYPTION_KEY"),
		TLSCertFile:                 get("TLS_CERT_FILE"),
		TLSKeyFile:                  get("TLS_KEY_FILE"),
		MigrationsDir:               get("AUTHAPI_MIGRATIONS_DIR"),
	}, nil
}

func splitPerms(s string) []string {
	parts := strings.FieldsFunc(s, func(r rune) bool {
		return r == ',' || r == ' ' || r == ';'
	})
	seen := map[string]bool{}
	out := []string{}
	for _, p := range parts {
		if !seen[p] {
			seen[p] = true
			out = append(out, p)
		}
	}
	return out
}

// configPath resolves the location of the config file.
func configPath(args []string) (string, error) {
	if len(args) > 1 {
		return args[1], nil
	}
	if p := os.Getenv("AUTHAPI_CONFIG"); p != "" {
		return p, nil
	}
	return "config.env", nil
}
