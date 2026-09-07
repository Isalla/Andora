package main

import (
	"fmt"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// Service describes one managed Andora component. Each service has a
// unique key (operator-chosen), a systemd unit, and an optional
// health/status endpoint the agent probes. Voice is not listed here
// because it is a later release and does not require an agent rebuild.
type Service struct {
	Key        string
	Unit       string
	HealthURL  string // full base URL, e.g. http://127.0.0.1:8082
	HealthPath string // default "/health"
}

// Config is loaded from an EnvironmentFile-style file. Optional
// argument:
//
//	./agent [/path/to/config.env]
//
// The path may also be given via AGENT_CONFIG. If neither is
// given, "config.env" next to the binary is used.
type Config struct {
	Port     int
	BindHost string
	Token    string

	Services []Service

	SystemctlPath  string
	JournalctlPath string
	SudoPath       string
	UseSudo        bool

	LogLineLimit int
	LogByteLimit int
	ProbeTimeout time.Duration

	// Server TLS (optional, like other Andora Go services).
	TLSCertFile string
	TLSKeyFile  string

	// mTLS structural preparation for a future central-panel
	// connection. All three must be set together, or all empty.
	ClientCAFile   string
	ClientCertFile string
	ClientKeyFile  string
}

func intEnv(env map[string]string, key string, def int) int {
	v := strings.TrimSpace(env[key])
	if v == "" {
		return def
	}
	n, err := strconv.Atoi(v)
	if err != nil {
		return def
	}
	return n
}

func boolEnv(env map[string]string, key string, def bool) bool {
	v := strings.ToLower(strings.TrimSpace(env[key]))
	if v == "" {
		return def
	}
	return v == "1" || v == "true" || v == "yes"
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

var validKey = regexp.MustCompile(`^[A-Z0-9_-]+$`)
var validUnit = regexp.MustCompile(`^[A-Za-z0-9._-]+\.service$`)

// loadConfig validates and builds the full configuration. Missing
// required values abort the service with a clear error.
func loadConfig(path string) (*Config, error) {
	env, err := parseEnvFile(path)
	if err != nil {
		return nil, fmt.Errorf("read config %s: %w", path, err)
	}

	port := intEnv(env, "AGENT_PORT", 9443)
	bindHost := strings.TrimSpace(env["AGENT_BIND_HOST"])
	if _, err := listenHosts(bindHost, port); err != nil {
		return nil, fmt.Errorf("AGENT_BIND_HOST: %w", err)
	}

	token := strings.TrimSpace(env["AGENT_TOKEN"])
	if token == "" {
		return nil, fmt.Errorf("AGENT_TOKEN is required")
	}

	services, err := parseServices(env)
	if err != nil {
		return nil, err
	}

	tlsCert := strings.TrimSpace(env["TLS_CERT_FILE"])
	tlsKey := strings.TrimSpace(env["TLS_KEY_FILE"])
	caFile := strings.TrimSpace(env["AGENT_CA_FILE"])
	clientCert := strings.TrimSpace(env["AGENT_CLIENT_CERT_FILE"])
	clientKey := strings.TrimSpace(env["AGENT_CLIENT_KEY_FILE"])

	if (tlsCert == "") != (tlsKey == "") {
		return nil, fmt.Errorf("TLS requires both TLS_CERT_FILE and TLS_KEY_FILE")
	}
	mTLS := caFile != "" || clientCert != "" || clientKey != ""
	if mTLS && (caFile == "" || clientCert == "" || clientKey == "") {
		return nil, fmt.Errorf("mTLS requires AGENT_CA_FILE, AGENT_CLIENT_CERT_FILE and AGENT_CLIENT_KEY_FILE")
	}
	// Reject mTLS without server TLS: the configuration would otherwise
	// silently degrade to token-only plain HTTP while the operator
	// assumes client-certificate verification is active.
	if mTLS && tlsCert == "" {
		return nil, fmt.Errorf("mTLS configuration requires TLS_CERT_FILE and TLS_KEY_FILE")
	}

	probeTimeout := time.Duration(intEnv(env, "AGENT_PROBE_TIMEOUT_MS", 1500)) * time.Millisecond
	if probeTimeout < 100*time.Millisecond {
		probeTimeout = 100 * time.Millisecond
	}

	logLineLimit := intEnv(env, "AGENT_LOG_LINE_LIMIT", 200)
	if logLineLimit < 1 {
		logLineLimit = 1
	}
	if logLineLimit > 10000 {
		logLineLimit = 10000
	}

	logByteLimit := intEnv(env, "AGENT_LOG_BYTE_LIMIT", 128*1024)
	if logByteLimit < 1024 {
		logByteLimit = 1024
	}

	systemctl := strings.TrimSpace(env["AGENT_SYSTEMCTL"])
	if systemctl == "" {
		systemctl = "/usr/bin/systemctl"
	}
	journalctl := strings.TrimSpace(env["AGENT_JOURNALCTL"])
	if journalctl == "" {
		journalctl = "/usr/bin/journalctl"
	}
	sudo := strings.TrimSpace(env["AGENT_SUDO"])
	if sudo == "" {
		sudo = "/usr/bin/sudo"
	}

	return &Config{
		Port:           port,
		BindHost:       bindHost,
		Token:          token,
		Services:       services,
		SystemctlPath:  systemctl,
		JournalctlPath: journalctl,
		SudoPath:       sudo,
		UseSudo:        boolEnv(env, "AGENT_SUDO_ENABLED", true),
		LogLineLimit:   logLineLimit,
		LogByteLimit:   logByteLimit,
		ProbeTimeout:   probeTimeout,
		TLSCertFile:    tlsCert,
		TLSKeyFile:     tlsKey,
		ClientCAFile:   caFile,
		ClientCertFile: clientCert,
		ClientKeyFile:  clientKey,
	}, nil
}

// parseServices extracts AGENT_SERVICE_<KEY>_UNIT / _URL / _HEALTH
// entries from the environment.
func parseServices(env map[string]string) ([]Service, error) {
	// Collect service keys referenced in AGENT_SERVICES.
	keysRaw := strings.TrimSpace(env["AGENT_SERVICES"])
	if keysRaw == "" {
		return nil, fmt.Errorf("AGENT_SERVICES is required")
	}
	var keys []string
	seen := map[string]bool{}
	for _, k := range strings.Split(keysRaw, ",") {
		k = strings.ToUpper(strings.TrimSpace(k))
		if k == "" {
			continue
		}
		if !validKey.MatchString(k) {
			return nil, fmt.Errorf("AGENT_SERVICES key %q: must match [A-Z0-9_-]+", k)
		}
		if seen[k] {
			return nil, fmt.Errorf("AGENT_SERVICES duplicate key %q", k)
		}
		seen[k] = true
		keys = append(keys, k)
	}
	if len(keys) == 0 {
		return nil, fmt.Errorf("AGENT_SERVICES must contain at least one key")
	}

	var services []Service
	for _, k := range keys {
		prefix := "AGENT_SERVICE_" + k + "_"
		unit := strings.TrimSpace(env[prefix+"UNIT"])
		if unit == "" {
			return nil, fmt.Errorf("AGENT_SERVICE_%s_UNIT is required", k)
		}
		if !validUnit.MatchString(unit) {
			return nil, fmt.Errorf("AGENT_SERVICE_%s_UNIT %q: must match <name>.service", k, unit)
		}

		healthURL, err := normalizeURLHost(strings.TrimSpace(env[prefix+"URL"]))
		if err != nil {
			return nil, fmt.Errorf("AGENT_SERVICE_%s_URL: %w", k, err)
		}

		healthPath := strings.TrimSpace(env[prefix+"HEALTH_PATH"])
		if healthPath == "" {
			healthPath = "/health"
		}
		if !strings.HasPrefix(healthPath, "/") {
			healthPath = "/" + healthPath
		}

		svcKey := strings.ToLower(k)
		services = append(services, Service{
			Key:        svcKey,
			Unit:       unit,
			HealthURL:  healthURL,
			HealthPath: healthPath,
		})
	}

	// Reject duplicate units.
	byUnit := map[string]string{}
	for _, s := range services {
		if prev, dup := byUnit[s.Unit]; dup {
			return nil, fmt.Errorf("duplicate unit %q (keys %q and %q)", s.Unit, prev, s.Key)
		}
		byUnit[s.Unit] = s.Key
	}

	return services, nil
}

// configPath resolves the location of the config file.
func configPath(args []string) (string, error) {
	if len(args) > 1 {
		return args[1], nil
	}
	if p := os.Getenv("AGENT_CONFIG"); p != "" {
		return p, nil
	}
	return "config.env", nil
}
