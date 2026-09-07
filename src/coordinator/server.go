package main

import (
	"context"
	"crypto/tls"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

const (
	// readHeaderTimeout bounds how long the http server waits for headers.
	readHeaderTimeout = 15 * time.Second
	// probeInterval is how often /status refreshes the Ollama flag.
	probeInterval = 10 * time.Second
)

// Server is the coordinator service: a file-backed KI queue with
// Ollama as the single inference backend (docs/Coordinator.md).
type Server struct {
	cfg    *Config
	store  *fileStore
	queue  *queueService
	ollama *ollamaClient
	realms *realmClient
	rl     *rateLimit
	start  time.Time
}

func newServer(cfg *Config, store *fileStore) *Server {
	oc := newOllamaClient(cfg)
	rc := newRealmClient(cfg)
	q := newQueueService(cfg, store, oc, rc)
	return &Server{
		cfg:    cfg,
		store:  store,
		queue:  q,
		ollama: oc,
		realms: rc,
		rl:     newRateLimit(cfg),
		start:  time.Now(),
	}
}

// handler assembles the middleware + routing chain.
func (s *Server) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", s.handleHealth)
	mux.HandleFunc("/status", s.handleStatus)
	mux.HandleFunc("/v1/jobs", s.handleSubmitJob)
	mux.HandleFunc("/v1/jobs/", s.handleQueryJob)
	return loggingMiddleware(noCacheHandler(mux))
}

// listeners opens one or more TCP listeners according to the
// configured bind host and port, TLS-enabled iff both cert and key
// are configured. Dual bind hosts yield one listener per family.
func (s *Server) listeners() ([]net.Listener, error) {
	var tlsCfg *tls.Config
	if s.cfg.TLSCertFile != "" && s.cfg.TLSKeyFile != "" {
		cert, err := tls.LoadX509KeyPair(s.cfg.TLSCertFile, s.cfg.TLSKeyFile)
		if err != nil {
			return nil, fmt.Errorf("load TLS cert/key: %w", err)
		}
		tlsCfg = &tls.Config{MinVersion: tls.VersionTLS12, Certificates: []tls.Certificate{cert}}
	}
	plans, err := listenHosts(s.cfg.BindHost, s.cfg.Port)
	if err != nil {
		return nil, err
	}
	return openListeners(plans, tlsCfg)
}

// probeLoop refreshes the status flag so operators can see whether
// Ollama is reachable even when no jobs are running.
func (s *Server) probeLoop(ctx context.Context) {
	t := time.NewTicker(probeInterval)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			s.queue.probeOllama()
		}
	}
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
	if (cfg.TLSCertFile == "") != (cfg.TLSKeyFile == "") {
		fmt.Fprintln(os.Stderr, "config: TLS requires both TLS_CERT_FILE and TLS_KEY_FILE")
		os.Exit(1)
	}

	store := newFileStore(cfg.DataDir)
	if err := store.Init(); err != nil {
		fmt.Fprintln(os.Stderr, "queue:", err)
		os.Exit(1)
	}
	srv := newServer(cfg, store)
	srv.queue.requeueStale()

	lns, err := srv.listeners()
	if err != nil {
		fmt.Fprintln(os.Stderr, "listen:", err)
		os.Exit(1)
	}
	for _, ln := range lns {
		fmt.Printf("coordinator listening on %s (ollama %s, model %s)\n",
			ln.Addr().String(), cfg.OllamaURL, cfg.OllamaModel)
	}

	dispatchCtx, stopDispatch := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		srv.queue.run(dispatchCtx)
		close(done)
	}()
	go srv.probeLoop(dispatchCtx)

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
		fmt.Println("coordinator shutting down")
	}
	// Graceful shutdown (§19): stop accepting new jobs, finish the
	// running one, then stop HTTP and persist a consistent queue.
	stopDispatch()
	<-done
	httpCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_ = httpServer.Shutdown(httpCtx)
}
