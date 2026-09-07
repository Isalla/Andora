package main

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"
)

const (
	readHeaderTimeout = 15 * time.Second
	agentVersion      = "0.1.0"
)

// Server is the andora-agent management daemon.
type Server struct {
	cfg   *Config
	ctl   *Controller
	rl    *rateLimit
	start time.Time
}

func newServer(cfg *Config, ctl *Controller) *Server {
	return &Server{
		cfg:   cfg,
		ctl:   ctl,
		rl:    newRateLimit(10, 120),
		start: time.Now(),
	}
}

// handler assembles the middleware + routing chain.
func (s *Server) handler() http.Handler {
	mux := http.NewServeMux()

	// Open endpoints (no auth).
	mux.HandleFunc("/health", s.handleHealth)
	mux.HandleFunc("/status", s.handleStatus)

	// Management endpoints (auth required).
	mux.HandleFunc("/api/v1/services", requireAuth(s.cfg.Token, s.handleServicesList))
	mux.HandleFunc("/api/v1/services/", requireAuth(s.cfg.Token, s.handleServiceRoute))

	return loggingMiddleware(noCacheHandler(mux))
}

// --- open endpoints ---

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func (s *Server) handleStatus(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"status":   "ok",
		"version":  agentVersion,
		"uptime":   time.Since(s.start).Round(time.Second).String(),
		"services": len(s.cfg.Services),
	})
}

// --- management endpoints ---

func (s *Server) handleServicesList(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	type svcEntry struct {
		Key     string `json:"key"`
		Unit    string `json:"unit"`
		Active  bool   `json:"active"`
		State   string `json:"state"`
		Healthy bool   `json:"healthy"`
	}
	out := make([]svcEntry, 0, len(s.cfg.Services))
	for _, svc := range s.cfg.Services {
		state := s.ctl.State(svc.Unit)
		healthy, _ := s.ctl.Healthy(svc)
		out = append(out, svcEntry{
			Key:     svc.Key,
			Unit:    svc.Unit,
			Active:  state == "active",
			State:   state,
			Healthy: healthy,
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"status":   "ok",
		"services": out,
	})
}

func (s *Server) handleServiceRoute(w http.ResponseWriter, r *http.Request) {
	// Parse /api/v1/services/{key}/{action}
	rest := strings.TrimPrefix(r.URL.Path, "/api/v1/services/")
	parts := strings.SplitN(rest, "/", 2)
	key := strings.TrimSpace(parts[0])
	action := ""
	if len(parts) > 1 {
		action = strings.TrimSpace(parts[1])
	}
	if key == "" {
		writeError(w, http.StatusBadRequest, "missing service key")
		return
	}
	svc, err := s.ctl.service(key)
	if err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	// Rate limit per key.
	if ok, retry := s.rl.allow("svc:" + key); !ok {
		w.Header().Set("Retry-After", fmt.Sprintf("%d", retry))
		writeError(w, http.StatusTooManyRequests, "rate limit exceeded")
		return
	}

	switch action {
	case "":
		// GET /api/v1/services/{key} — service detail.
		s.handleServiceDetail(w, r, svc)
	case "start", "stop", "restart":
		// POST /api/v1/services/{key}/{action}.
		s.handleServiceAction(w, r, svc, action)
	case "logs":
		s.handleServiceLogs(w, r, svc)
	case "health":
		s.handleServiceHealth(w, r, svc)
	case "version":
		s.handleServiceVersion(w, r, svc)
	default:
		writeError(w, http.StatusNotFound, "unknown endpoint: "+action)
	}
}

func (s *Server) handleServiceDetail(w http.ResponseWriter, r *http.Request, svc Service) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	state := s.ctl.State(svc.Unit)
	healthy, _ := s.ctl.Healthy(svc)
	version := s.ctl.Version(svc)
	writeJSON(w, http.StatusOK, map[string]any{
		"status":     "ok",
		"key":        svc.Key,
		"unit":       svc.Unit,
		"state":      state,
		"active":     state == "active",
		"healthy":    healthy,
		"version":    version,
		"health_url": svc.HealthURL + svc.HealthPath,
	})
}

func (s *Server) handleServiceAction(w http.ResponseWriter, r *http.Request, svc Service, action string) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	output, err := s.ctl.Action(action, svc.Unit)
	if err != nil {
		writeError(w, http.StatusBadGateway, "action failed")
		return
	}
	// Return the post-action state.
	state := s.ctl.State(svc.Unit)
	writeJSON(w, http.StatusOK, map[string]any{
		"status": "ok",
		"key":    svc.Key,
		"unit":   svc.Unit,
		"action": action,
		"state":  state,
		"output": output,
	})
}

func (s *Server) handleServiceLogs(w http.ResponseWriter, r *http.Request, svc Service) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	lines := parseIntQuery(r, "lines", s.cfg.LogLineLimit, 1, s.cfg.LogLineLimit)
	logLines, truncated := s.ctl.Logs(svc.Unit, lines)
	if logLines == nil {
		logLines = []string{}
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"status":    "ok",
		"key":       svc.Key,
		"unit":      svc.Unit,
		"lines":     logLines,
		"returned":  len(logLines),
		"requested": lines,
		"truncated": truncated,
	})
}

func (s *Server) handleServiceHealth(w http.ResponseWriter, r *http.Request, svc Service) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	start := time.Now()
	healthy, detail := s.ctl.Healthy(svc)
	latency := time.Since(start).Milliseconds()
	writeJSON(w, http.StatusOK, map[string]any{
		"status":     "ok",
		"key":        svc.Key,
		"unit":       svc.Unit,
		"healthy":    healthy,
		"latency_ms": latency,
		"detail":     detail,
	})
}

func (s *Server) handleServiceVersion(w http.ResponseWriter, r *http.Request, svc Service) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	version := s.ctl.Version(svc)
	writeJSON(w, http.StatusOK, map[string]any{
		"status":  "ok",
		"key":     svc.Key,
		"unit":    svc.Unit,
		"version": version,
	})
}

// --- listeners and main ---

func (s *Server) listeners() ([]net.Listener, error) {
	var tlsCfg *tls.Config
	if s.cfg.TLSCertFile != "" && s.cfg.TLSKeyFile != "" {
		cert, err := tls.LoadX509KeyPair(s.cfg.TLSCertFile, s.cfg.TLSKeyFile)
		if err != nil {
			return nil, fmt.Errorf("load TLS cert/key: %w", err)
		}
		tlsCfg = &tls.Config{MinVersion: tls.VersionTLS12, Certificates: []tls.Certificate{cert}}
		// mTLS structural preparation: when a client CA is
		// configured, require and verify client certificates.
		if s.cfg.ClientCAFile != "" {
			caPEM, err := os.ReadFile(s.cfg.ClientCAFile)
			if err != nil {
				return nil, fmt.Errorf("load client CA: %w", err)
			}
			pool := x509.NewCertPool()
			if !pool.AppendCertsFromPEM(caPEM) {
				return nil, fmt.Errorf("parse client CA: no valid PEM")
			}
			tlsCfg.ClientCAs = pool
			tlsCfg.ClientAuth = tls.RequireAndVerifyClientCert
		}
	}
	plans, err := listenHosts(s.cfg.BindHost, s.cfg.Port)
	if err != nil {
		return nil, err
	}
	return openListeners(plans, tlsCfg)
}

func main() {
	cppath, err := configPath(os.Args)
	if err != nil {
		fmt.Fprintln(os.Stderr, "config path:", err)
		os.Exit(1)
	}
	cfg, err := loadConfig(cppath)
	if err != nil {
		fmt.Fprintln(os.Stderr, "config:", err)
		os.Exit(1)
	}

	ctl := NewController(cfg, nil)
	srv := newServer(cfg, ctl)

	lns, err := srv.listeners()
	if err != nil {
		fmt.Fprintln(os.Stderr, "listen:", err)
		os.Exit(1)
	}
	for _, ln := range lns {
		fmt.Printf("andora-agent listening on %s (services: %d)\n",
			ln.Addr().String(), len(cfg.Services))
	}

	httpServer := &http.Server{Handler: srv.handler(), ReadHeaderTimeout: readHeaderTimeout}
	errCh := make(chan error, len(lns))
	for _, ln := range lns {
		ln := ln
		go func() { errCh <- httpServer.Serve(ln) }()
	}
	sig := make(chan os.Signal, 2)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	select {
	case err := <-errCh:
		if err != nil {
			fmt.Fprintln(os.Stderr, "http:", err)
			os.Exit(1)
		}
	case <-sig:
		fmt.Println("andora-agent shutting down")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_ = httpServer.Shutdown(ctx)
}
