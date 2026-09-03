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

// Config is loaded from an EnvironmentFile-style file. Optional
// argument:
//
//	./authapi [/path/to/config.env]
//
// The path may also be given via AUTHAPI_CONFIG. If neither is given,
// "config.env" next to the binary is used.
type Config struct {
	Port         int
	AuthDB       AuthDBConfig
	Services     map[string]ServiceCred
	ArgonTime    uint32
	ArgonMemory  uint32
	ArgonThreads uint8
	// Security parameters.
	SessionTTL      time.Duration
	HandoffTTL      time.Duration
	RecoveryTTL     time.Duration
	CallbackURLHost string
	NewAccountBan   time.Duration
	// Rate limiting: burst allowed concurrently + requests per minute.
	RateLimitBurst  int
	RateLimitPerMin int
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
		AuthDB:          db,
		Services:        svc,
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
		EncryptionKey:   get("ENCRYPTION_KEY"),
		TLSCertFile:     get("TLS_CERT_FILE"),
		TLSKeyFile:      get("TLS_KEY_FILE"),
		MigrationsDir:   get("AUTHAPI_MIGRATIONS_DIR"),
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
