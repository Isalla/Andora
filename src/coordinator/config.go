package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

// Config is loaded from an EnvironmentFile-style file. Optional
// argument:
//
//	./coordinator [/path/to/config.env]
//
// The path may also be given via COORDINATOR_CONFIG. If neither is
// given, "config.env" next to the binary is used.
//
// The coordinator holds NO database credentials (docs/Coordinator.md
// section 2). It only ever talks to Ollama (input side) and to the
// realms that submitted jobs (result callback / job queries).
type Config struct {
	Port     int
	BindHost string
	// DataDir is the persistent queue area. Layout (all relative to
	// DataDir, never system /tmp):
	//   queue/jobs/<timestamp>_<type>-<id>.json   one file per job
	//   queue/queue.json                          current job order
	//   queue/quarantine/                         broken/missing files
	//   queue/done/                               terminal jobs (status)
	//   queue/jobs/.tmp-*                         atomic-write temp files
	DataDir string

	// FilterDir is the directory with the language filter files
	// (<lang>.input.txt / <lang>.output.txt, docs/Coordinator.md
	// section 9). All language files that exist are loaded at startup
	// and always active, independent of the client/player locale.
	FilterDir string

	// CoordinatorServiceID/Secret sign the coordinator's own result
	// callbacks to realms (only used when a realm's result URL is set).
	CoordinatorServiceID string
	CoordinatorSecret    string

	// Realms maps the realm public id (the value used in job payloads'
	// realm_id) to its credential + optional result endpoint.
	Realms map[string]RealmCred

	// Ollama settings.
	OllamaURL     string
	OllamaModel   string
	NumCtx        int
	OllamaTimeout time.Duration
	Temperature   float64
	TopP          float64
	TopK          int

	// Queue / worker tuning.
	Workers      int
	QueueMaxSize int
	// PlayerCooldown is the minimum interval between two free KI
	// requests of the same player (docs/Coordinator.md section 6).
	PlayerCooldown time.Duration
	// MaxTextChars limits free player input (section 7).
	MaxTextChars int
	// MaxAttempts is the correction/retry budget per job (section 11).
	MaxAttempts int
	// ContextReservedTokens is the answer space kept free inside the
	// Ollama context window (section 8).
	ContextReservedTokens int

	// Priorities are configurable weights per job type (section 5).
	// Within the same weight jobs are processed by arrival order (the
	// timestamp in the job file name).
	Priorities map[string]int

	// InputDenyWords is a comma-separated list of content rules applied
	// before anything reaches Ollama (section 9). Empty = no word rules.
	InputDenyWords []string
	// OutputDenyWords is a comma-separated list of world/plausibility
	// rules applied to Ollama answers (section 10).
	OutputDenyWords []string
	// AllowedActionKeys is the closed set of keys a structured action
	// JSON answer may contain (section 23 of docs/ai_system.md).
	AllowedActionKeys []string

	// Rate limiting for signed service calls.
	RateLimitBurst  int
	RateLimitPerMin int
	// Optional TLS (certificate + key path; empty disables).
	TLSCertFile string
	TLSKeyFile  string

	// modOnce/modValue hold the lazily loaded moderation filter state
	// (moderation.go). The coordinator does NOT honor X-Forwarded-For /
	// X-Real-IP and has no per-client-IP rate limiting, so there is no
	// trusted-proxy configuration here.
	modOnce  sync.Once
	modValue *moderation
}

// RealmCred is one registered realm: the public realm id used in job
// payloads, the service credential the realm presents when calling the
// coordinator, and an optional result endpoint the coordinator calls
// back on terminal job status. The result URL is configured by the
// operator; when it is empty, realms poll GET /v1/jobs/{id} instead.
type RealmCred struct {
	ID          string
	ServiceID   string
	Secret      string
	Permissions []string
	ResultURL   string
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

func floatEnv(env map[string]string, key string, def float64) float64 {
	v := env[key]
	if v == "" {
		return def
	}
	n, err := strconv.ParseFloat(v, 64)
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

func splitList(s string) []string {
	parts := strings.FieldsFunc(s, func(r rune) bool {
		return r == ',' || r == ' ' || r == ';'
	})
	var out []string
	for _, p := range parts {
		p = strings.ToLower(strings.TrimSpace(p))
		if p != "" {
			out = append(out, p)
		}
	}
	return out
}

// splitPerms mirrors src/api/config.go: comma/space/semicolon-separated
// permission list, deduplicated.
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

// loadConfig validates and builds the full configuration. Missing
// required values abort the service with a clear error.
func loadConfig(path string) (*Config, error) {
	env, err := parseEnvFile(path)
	if err != nil {
		return nil, fmt.Errorf("read config %s: %w", path, err)
	}
	get := func(k string) string { return env[k] }

	port := intEnv(env, "COORDINATOR_PORT", 8082)
	bindHost := strings.TrimSpace(get("COORDINATOR_BIND_HOST"))
	if _, err := listenHosts(bindHost, port); err != nil {
		return nil, fmt.Errorf("COORDINATOR_BIND_HOST: %w", err)
	}

	dataDir := strings.TrimSpace(get("COORDINATOR_DATA_DIR"))
	if dataDir == "" {
		dataDir = "data"
	}
	abs, err := filepath.Abs(dataDir)
	if err == nil {
		dataDir = abs
	}

	filterDir := strings.TrimSpace(get("COORDINATOR_FILTER_DIR"))
	if filterDir == "" {
		filterDir = "filters"
	}
	if abs, err := filepath.Abs(filterDir); err == nil {
		filterDir = abs
	}

	ollamaURL, err := normalizeURLHost(strings.TrimRight(get("OLLAMA_URL"), "/"))
	if err != nil {
		return nil, err
	}
	if ollamaURL == "" {
		return nil, fmt.Errorf("OLLAMA_URL is required")
	}
	model := strings.TrimSpace(get("OLLAMA_MODEL"))
	if model == "" {
		return nil, fmt.Errorf("OLLAMA_MODEL is required")
	}

	svcID := get("COORDINATOR_SERVICE_ID")
	svcSecret := get("COORDINATOR_SERVICE_SECRET")

	realms, err := parseRealms(env)
	if err != nil {
		return nil, err
	}

	priorities := map[string]int{}
	for _, p := range defaultPriorities {
		key := "PRIORITY_" + strings.ToUpper(strings.ReplaceAll(p.Type, "-", "_"))
		priorities[p.Type] = intEnv(env, key, p.Weight)
	}

	return &Config{
		Port:                 port,
		BindHost:             bindHost,
		DataDir:              dataDir,
		FilterDir:            filterDir,
		CoordinatorServiceID: svcID,
		CoordinatorSecret:    svcSecret,
		Realms:               realms,

		OllamaURL:     ollamaURL,
		OllamaModel:   model,
		NumCtx:        intEnv(env, "OLLAMA_NUM_CTX", 8192),
		OllamaTimeout: time.Duration(intEnv(env, "OLLAMA_TIMEOUT_MS", 15000)) * time.Millisecond,
		Temperature:   floatEnv(env, "OLLAMA_TEMPERATURE", 0.8),
		TopP:          floatEnv(env, "OLLAMA_TOP_P", 0.95),
		TopK:          intEnv(env, "OLLAMA_TOP_K", 64),

		Workers:               intEnv(env, "COORDINATOR_WORKERS", 2),
		QueueMaxSize:          intEnv(env, "QUEUE_MAX_SIZE", 200),
		PlayerCooldown:        time.Duration(intEnv(env, "PLAYER_COOLDOWN_SECONDS", 5)) * time.Second,
		MaxTextChars:          intEnv(env, "MAX_TEXT_CHARS", 500),
		MaxAttempts:           intEnv(env, "MAX_CORRECTION_ATTEMPTS", 5),
		ContextReservedTokens: intEnv(env, "CONTEXT_RESERVED_TOKENS", 1536),

		Priorities: priorities,

		InputDenyWords:    splitList(get("INPUT_DENY_WORDS")),
		OutputDenyWords:   splitList(get("OUTPUT_DENY_WORDS")),
		AllowedActionKeys: splitList(get("ALLOWED_ACTION_KEYS")),

		RateLimitBurst:  intEnv(env, "RATE_LIMIT_BURST", 10),
		RateLimitPerMin: intEnv(env, "RATE_LIMIT_PER_MIN", 120),
		TLSCertFile:     get("TLS_CERT_FILE"),
		TLSKeyFile:      get("TLS_KEY_FILE"),
	}, nil
}

// parseRealms registers every SERVICE_<name>_ID/_SECRET/_PERMISSIONS
// tuple that also has a matching REALM_<name>_ID (the realm public id)
// and optionally REALM_<name>_RESULT_URL. A service credential without
// a realm mapping is an error: the coordinator must not accept jobs
// from an unrecognized realm. Permission names are validated below
// (security.go) when the request is authorized.
func parseRealms(env map[string]string) (map[string]RealmCred, error) {
	out := map[string]RealmCred{}
	for k, v := range env {
		if !strings.HasPrefix(k, "SERVICE_") || !strings.HasSuffix(k, "_ID") {
			continue
		}
		base := strings.TrimPrefix(k, "SERVICE_")
		base = strings.TrimSuffix(base, "_ID")
		svcID := v
		secret := env["SERVICE_"+base+"_SECRET"]
		perms := env["SERVICE_"+base+"_PERMISSIONS"]
		realmID := env["REALM_"+base+"_ID"]
		if realmID == "" {
			return nil, fmt.Errorf("service %q has no REALM_%s_ID mapping", base, base)
		}
		if svcID == "" || secret == "" || perms == "" {
			return nil, fmt.Errorf("realm %q: SERVICE_%s_ID/_SECRET/_PERMISSIONS are all required", realmID, base)
		}
		resultURL := strings.TrimSpace(env["REALM_"+base+"_RESULT_URL"])
		if resultURL != "" {
			u, err := normalizeURLHost(resultURL)
			if err != nil {
				return nil, fmt.Errorf("REALM_%s_RESULT_URL: %w", base, err)
			}
			resultURL = u
		}
		if _, dup := out[realmID]; dup {
			return nil, fmt.Errorf("duplicate realm public id %q", realmID)
		}
		out[realmID] = RealmCred{
			ID:          realmID,
			ServiceID:   svcID,
			Secret:      secret,
			Permissions: splitPerms(perms),
			ResultURL:   resultURL,
		}
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("at least one registered realm (REALM_<name>_ID + SERVICE_<name>_*) is required")
	}
	return out, nil
}

type priorityDef struct {
	Type   string
	Weight int
}

// defaultPriorities is the base weighting from docs/Coordinator.md
// section 5. The concrete weights are configurable per priority key.
var defaultPriorities = []priorityDef{
	{Type: "raidboss", Weight: 100},
	{Type: "player", Weight: 90},
	{Type: "craft", Weight: 90},
	{Type: "npc", Weight: 80},
	{Type: "event", Weight: 70},
	{Type: "background", Weight: 30},
	{Type: "world", Weight: 10},
}

// hasPermission reports whether the realm credential carries the given
// permission (closed set, see security.go).
func (r RealmCred) hasPermission(p string) bool {
	for _, x := range r.Permissions {
		if x == p {
			return true
		}
	}
	return false
}

// configPath resolves the location of the config file.
func configPath(args []string) (string, error) {
	if len(args) > 1 {
		return args[1], nil
	}
	if p := os.Getenv("COORDINATOR_CONFIG"); p != "" {
		return p, nil
	}
	return "config.env", nil
}
