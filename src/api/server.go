package main

import (
	"context"
	"crypto/tls"
	"database/sql"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"sync"
	"syscall"
	"time"
)

const (
	// startTimeout bounds startup db work (connect + migrations).
	startTimeout = 20 * time.Second
	// readHeaderTimeout bounds how long the http server waits for headers.
	readHeaderTimeout = 15 * time.Second
	// verifyDBTimeout bounds database work during a verify request.
	verifyDBTimeout = 5 * time.Second
	// sessionStatsCloseGrace bounds how long shutdown waits for a
	// demand-triggered session collection before the pool is closed.
	sessionStatsCloseGrace = 2 * time.Second
)

// Server holds configuration, the auth-database pool, the data store
// and the process-wide rate limiter.
type Server struct {
	cfg   *Config
	db    *sql.DB
	store AuthStore
	rl    *rateLimit
	start time.Time

	// Session monitoring (P-33). sessionStatsMu guards the admission
	// decision (closing AND in-flight AND minimum distance) AND the
	// published snapshot together: separate atomics alone would not prove a
	// correct admission decision, because admission has to read and change
	// all of these facts at once. sessionStatsPub always points to an
	// immutable snapshot that is replaced, never mutated in place.
	sessionStatsMu          sync.Mutex
	sessionStatsPub         *sessionStatsSnapshot
	sessionStatsInFlight    bool
	sessionStatsNextAllowed time.Time
	sessionStatsAttempts    uint64
	// sessionStatsRunning counts the admitted-and-not-yet-finished attempts
	// and sessionStatsDone is closed by the LAST returning attempt. Both are
	// guarded by sessionStatsMu. A WaitGroup is deliberately NOT used: its
	// Add/Wait contract cannot express "admission is closed" and would
	// allow an Add after a Wait had already started.
	sessionStatsRunning int
	sessionStatsDone    chan struct{}
	// sessionStatsCancel aborts the currently running attempt. close() calls
	// it so a running collection is cancelled instead of only waited for.
	sessionStatsCancel context.CancelFunc
	// sessionStatsClosing is set once, under sessionStatsMu, at the beginning
	// of close(). From then on admission always refuses, which is what makes
	// the wait in close() finite: no new attempt can appear.
	sessionStatsClosing bool
	// sessionStatsPoolClosed guards the pool close so a repeated or concurrent
	// close() never closes the pool twice.
	sessionStatsPoolClosed bool
	// sessionStatsSignal is a test seam: the collector does one
	// non-blocking send after each publication. It stays nil in
	// production.
	sessionStatsSignal chan struct{}
}

// newServer opens the auth DB, applies pending migrations and wires
// the SQL store. Any failure aborts the service: it must not run if
// the schema is not at the latest version.
func newServer(cfg *Config) (*Server, error) {
	if cfg.EncryptionKey == "" {
		return nil, fmt.Errorf("ENCRYPTION_KEY is required")
	}
	if (cfg.TLSCertFile == "") != (cfg.TLSKeyFile == "") {
		return nil, fmt.Errorf("TLS requires both TLS_CERT_FILE and TLS_KEY_FILE")
	}
	ctx, cancel := context.WithTimeout(context.Background(), startTimeout)
	defer cancel()
	db, err := openAuthDB(ctx, cfg.AuthDB)
	if err != nil {
		return nil, err
	}
	if err := applyAuthMigrations(ctx, db, cfg); err != nil {
		db.Close()
		return nil, err
	}
	return &Server{cfg: cfg, db: db, store: newSQLStore(db), rl: newRateLimit(cfg), start: time.Now()}, nil
}

// close releases the database pool. A demand-triggered collection may still
// be running: it is cancelled first and gets a bounded grace period, so the
// pool is never closed underneath a running query and the wait itself never
// becomes unbounded.
//
// The admission decision is closed under the SAME lock that close() uses, so
// no new attempt can be admitted once closing has begun. That is what keeps
// the wait finite without a per-call waiter goroutine. A store that IGNORES
// its context is NOT force-terminated; the bounded grace applies and the pool
// is closed afterwards. A repeated or concurrent close() does nothing.
func (s *Server) close() {
	s.sessionStatsMu.Lock()
	if s.sessionStatsClosing {
		// Repeated or concurrent close: no second waiter, no second
		// cancellation and no second pool close.
		s.sessionStatsMu.Unlock()
		return
	}
	// From here on admission refuses every attempt.
	s.sessionStatsClosing = true
	cancel := s.sessionStatsCancel
	done := s.sessionStatsDone
	s.sessionStatsMu.Unlock()

	if cancel != nil {
		// Targeted abort of the running attempt. This is a cooperative
		// cancellation: it reaches a store that honours ctx (the production
		// *sql.DB does, via QueryRowContext).
		cancel()
	}
	if done != nil {
		select {
		case <-done:
		case <-time.After(sessionStatsCloseGrace):
			log.Printf("authapi close: session collection still running after %s, closing pool anyway",
				sessionStatsCloseGrace)
		}
	}

	s.sessionStatsMu.Lock()
	if s.sessionStatsPoolClosed {
		s.sessionStatsMu.Unlock()
		return
	}
	s.sessionStatsPoolClosed = true
	db := s.db
	s.sessionStatsMu.Unlock()
	if db != nil {
		db.Close()
	}
}

// ratePreAuth applies the per-client rate limit before service
// authentication (caller unknown, so the key is IP + path).
func (s *Server) ratePreAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		key := "ip:" + clientIP(r, s.cfg.TrustedProxies) + ":" + r.URL.Path
		if ok, retry := s.rl.allow(key); !ok {
			w.Header().Set("Retry-After", strconv.Itoa(retry))
			writeError(w, http.StatusTooManyRequests, "rate limit exceeded")
			return
		}
		next.ServeHTTP(w, r)
	})
}

// handler assembles the complete middleware + routing chain of the
// service. Tests build a Server directly and use the same chain.
func (s *Server) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", s.handleHealth)
	mux.HandleFunc("/status", s.handleStatus)
	mux.HandleFunc("/auth/verify", s.handleAuthVerify)
	mux.HandleFunc("/account/register", s.handleAccountRegister)
	mux.HandleFunc("/account/password/change", s.handlePasswordChange)
	mux.HandleFunc("/account/recovery/request", s.handleRecoveryRequest)
	mux.HandleFunc("/account/recovery/confirm", s.handleRecoveryConfirm)
	mux.HandleFunc("/account/permissions", s.handleAccountPermissions)
	mux.HandleFunc("/session/create", s.handleSessionCreate)
	mux.HandleFunc("/session/validate", s.handleSessionValidate)
	mux.HandleFunc("/session/revoke", s.handleSessionRevoke)
	// Batch-Statusabfrage fuer den Realm-Revocation-Poller (AUTH-02b).
	// Nutzt dieselbe Berechtigung wie /session/validate.
	mux.HandleFunc("/session/status/batch", s.handleSessionStatusBatch)
	mux.HandleFunc("/realms", s.handleRealms)
	mux.HandleFunc("/handoff/create", s.handleHandoffCreate)
	mux.HandleFunc("/handoff/validate", s.handleHandoffValidate)
	// LEGACY: kein separater Worldserver mehr (Kette Auth/API → Login → Realm).
	mux.HandleFunc("/world/authenticate", s.handleWorldAuthenticate)
	mux.HandleFunc("/world/heartbeat", s.handleWorldHeartbeat)
	mux.HandleFunc("/twofactor/status", s.handleTwofactorStatus)
	mux.HandleFunc("/twofactor/setup", s.handleTwofactorSetup)
	mux.HandleFunc("/twofactor/enable", s.handleTwofactorEnable)
	mux.HandleFunc("/twofactor/disable", s.handleTwofactorDisable)
	mux.HandleFunc("/twofactor/reset", s.handleTwofactorReset)
	mux.HandleFunc("/devices/list", s.handleDevicesList)
	mux.HandleFunc("/devices/revoke", s.handleDevicesRevoke)
	mux.HandleFunc("/security/events", s.handleSecurityEvents)
	mux.HandleFunc("/parental/status", s.handleParentalStatus)
	mux.HandleFunc("/parental/setup", s.handleParentalSetup)
	mux.HandleFunc("/parental/update", s.handleParentalUpdate)
	mux.HandleFunc("/parental/remove", s.handleParentalRemove)
	mux.HandleFunc("/parental/pin/change", s.handleParentalPinChange)
	mux.HandleFunc("/parental/pin/verify", s.handleParentalPinVerify)
	mux.HandleFunc("/parental/extension", s.handleParentalExtension)
	mux.HandleFunc("/parental/periods", s.handleParentalPeriods)
	mux.HandleFunc("/parental/periods/add", s.handleParentalPeriodAdd)
	mux.HandleFunc("/parental/periods/remove", s.handleParentalPeriodRemove)
	mux.HandleFunc("/parental/exceptions/add", s.handleParentalExceptionAdd)
	mux.HandleFunc("/parental/exceptions/remove", s.handleParentalExceptionRemove)
	mux.HandleFunc("/parental/notifications", s.handleParentalNotifications)
	mux.HandleFunc("/parental/notifications/deliver", s.handleParentalNotificationsDeliver)
	return loggingMiddleware(noCacheHandler(s.ratePreAuth(mux)))
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
	srv, err := newServer(cfg)
	if err != nil {
		fmt.Fprintln(os.Stderr, "auth db:", err)
		os.Exit(1)
	}
	defer srv.close()

	lns, err := srv.listeners()
	if err != nil {
		fmt.Fprintln(os.Stderr, "listen:", err)
		os.Exit(1)
	}
	for _, ln := range lns {
		if len(cfg.TLSCertFile) > 0 {
			fmt.Printf("authapi listening on %s (tls, db %s@%s:%d/%s)\n",
				ln.Addr().String(), cfg.AuthDB.User, cfg.AuthDB.Host, cfg.AuthDB.Port, cfg.AuthDB.Database)
		} else {
			fmt.Printf("authapi listening on %s (db %s@%s:%d/%s)\n",
				ln.Addr().String(), cfg.AuthDB.User, cfg.AuthDB.Host, cfg.AuthDB.Port, cfg.AuthDB.Database)
		}
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
		fmt.Println("authapi shutting down")
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := httpServer.Shutdown(ctx); err != nil {
			fmt.Fprintln(os.Stderr, "shutdown:", err)
			os.Exit(1)
		}
	}
}

// noCacheHandler sets no-store and no-cache headers so a proxy or
// client does not cache sensitive auth responses.
func noCacheHandler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate")
		w.Header().Set("Pragma", "no-cache")
		w.Header().Set("Expires", "0")
		next.ServeHTTP(w, r)
	})
}
