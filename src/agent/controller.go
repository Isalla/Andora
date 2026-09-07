package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os/exec"
	"strings"
	"time"
)

// Runner abstracts external command execution so the controller can
// be tested without real systemctl or journalctl.
type Runner interface {
	Run(args []string) (stdout, stderr string, exitCode int)
}

// execRunner calls real binaries via os/exec.
type execRunner struct{}

func (execRunner) Run(args []string) (string, string, int) {
	if len(args) == 0 {
		return "", "empty args", 1
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, args[0], args[1:]...)
	var outBuf, errBuf bytes.Buffer
	cmd.Stdout = &outBuf
	cmd.Stderr = &errBuf
	err := cmd.Run()
	code := 0
	if err != nil {
		if ee, ok := err.(*exec.ExitError); ok {
			code = ee.ProcessState.ExitCode()
		} else {
			return "", err.Error(), 1
		}
	}
	return strings.TrimSpace(outBuf.String()), strings.TrimSpace(errBuf.String()), code
}

// validVerbs is the closed set of allowed systemctl verbs.
var validVerbs = map[string]bool{"start": true, "stop": true, "restart": true}

// Controller manages a set of whitelisted Andora services via
// systemctl/journalctl with constrained arguments.
type Controller struct {
	cfg    *Config
	runner Runner
	client *http.Client

	// index: key→service and unit→key for fast lookup.
	byKey map[string]Service
}

// NewController builds a Controller. When runner is nil the real
// execRunner is used.
func NewController(cfg *Config, runner Runner) *Controller {
	if runner == nil {
		runner = execRunner{}
	}
	byKey := map[string]Service{}
	for _, s := range cfg.Services {
		byKey[s.Key] = s
	}
	return &Controller{
		cfg:    cfg,
		runner: runner,
		client: &http.Client{Timeout: cfg.ProbeTimeout},
		byKey:  byKey,
	}
}

// service returns the Service for key or an error.
func (c *Controller) service(key string) (Service, error) {
	s, ok := c.byKey[key]
	if !ok {
		return Service{}, fmt.Errorf("unknown service: %s", key)
	}
	return s, nil
}

// State returns the systemd ActiveState for a unit.
// No sudo required (read-only).
func (c *Controller) State(unit string) string {
	args := []string{c.cfg.SystemctlPath, "show", "--property=ActiveState", "--value", "--no-pager", unit}
	stdout, _, code := c.runner.Run(args)
	if code != 0 || stdout == "" {
		return "unknown"
	}
	return strings.ToLower(strings.TrimSpace(stdout))
}

// Action performs a systemctl verb (start/stop/restart) on a unit.
// Mutating operations require sudo unless UseSudo is false.
func (c *Controller) Action(verb, unit string) (string, error) {
	if !validVerbs[verb] {
		return "", fmt.Errorf("invalid verb %q", verb)
	}
	args := []string{}
	if c.cfg.UseSudo {
		args = append(args, c.cfg.SudoPath, "-n")
	}
	args = append(args, c.cfg.SystemctlPath, verb, unit)
	stdout, stderr, code := c.runner.Run(args)
	if code != 0 {
		detail := stderr
		if detail == "" {
			detail = stdout
		}
		return "", fmt.Errorf("systemctl %s %s failed (exit %d): %s", verb, unit, code, detail)
	}
	return stdout, nil
}

// Logs retrieves the last n lines from the journal for a unit,
// clamped to the configured limits, and truncated to LogByteLimit.
func (c *Controller) Logs(unit string, lines int) (logLines []string, truncated bool) {
	n := lines
	if n < 1 {
		n = 1
	}
	if n > c.cfg.LogLineLimit {
		n = c.cfg.LogLineLimit
	}
	args := []string{}
	if c.cfg.UseSudo {
		args = append(args, c.cfg.SudoPath, "-n")
	}
	// The unit is pinned as the LAST argument. The matching sudoers
	// rule therefore ends in an exact `-u <unit>` token (no trailing
	// wildcard), so no further journalctl arguments can be added.
	args = append(args, c.cfg.JournalctlPath, "--no-pager", "--lines", fmt.Sprintf("%d", n), "-u", unit)
	stdout, _, code := c.runner.Run(args)
	if code != 0 || stdout == "" {
		return nil, false
	}
	raw := stdout
	if len(raw) > c.cfg.LogByteLimit {
		raw = raw[:c.cfg.LogByteLimit]
		truncated = true
	}
	logLines = strings.Split(raw, "\n")
	// Remove trailing empty line from journal output.
	if len(logLines) > 0 && logLines[len(logLines)-1] == "" {
		logLines = logLines[:len(logLines)-1]
	}
	if len(logLines) > n {
		logLines = logLines[len(logLines)-n:]
		truncated = true
	}
	return logLines, truncated
}

// Healthy probes the service health endpoint. Returns healthy flag
// and a detail string for the API response.
func (c *Controller) Healthy(svc Service) (healthy bool, detail string) {
	if svc.HealthURL == "" {
		return false, "no health URL configured"
	}
	url := strings.TrimRight(svc.HealthURL, "/") + svc.HealthPath
	resp, err := c.client.Get(url)
	if err != nil {
		return false, fmt.Sprintf("request failed: %v", err)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 64*1024))
	if err != nil {
		return false, fmt.Sprintf("read body failed: %v", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return false, fmt.Sprintf("http %d", resp.StatusCode)
	}
	var data map[string]any
	if err := json.Unmarshal(body, &data); err != nil {
		return false, "non-JSON response"
	}
	// Accept the Go convention {"status":"ok"} and the realm
	// convention {"ok":true}.
	if v, ok := data["status"].(string); ok && v == "ok" {
		return true, "ok"
	}
	if v, ok := data["ok"].(bool); ok && v {
		return true, "ok"
	}
	return false, "unexpected health payload"
}

// Version queries the service /status endpoint for a version field.
// Returns "unknown" when no version is available.
func (c *Controller) Version(svc Service) string {
	if svc.HealthURL == "" {
		return "unknown"
	}
	// Derive /status URL from the health URL base.
	statusURL := strings.TrimRight(svc.HealthURL, "/")
	if strings.HasSuffix(statusURL, svc.HealthPath) {
		statusURL = strings.TrimSuffix(statusURL, svc.HealthPath) + "/status"
	} else {
		parsed, err := parseURL(svc.HealthURL)
		if err != nil {
			return "unknown"
		}
		statusURL = parsed.Scheme + "://" + parsed.Host + "/status"
	}
	resp, err := c.client.Get(statusURL)
	if err != nil {
		return "unknown"
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 64*1024))
	if err != nil {
		return "unknown"
	}
	var data map[string]any
	if err := json.Unmarshal(body, &data); err != nil {
		return "unknown"
	}
	if v, ok := data["version"].(string); ok && v != "" {
		return v
	}
	return "unknown"
}
