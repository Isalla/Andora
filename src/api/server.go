package main

import (
	"context"
	"crypto/tls"
	"database/sql"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strconv"
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
)

// Server holds configuration, the auth-database pool, the data store
// and the process-wide rate limiter.
type Server struct {
	cfg   *Config
	db    *sql.DB
	store AuthStore
	rl    *rateLimit
	start time.Time
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

// close releases the database pool.
func (s *Server) close() {
	if s.db != nil {
		s.db.Close()
	}
}

// ratePreAuth applies the per-client rate limit before service
// authentication (caller unknown, so the key is IP + path).
func (s *Server) ratePreAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		key := "ip:" + remoteAddr(r) + ":" + r.URL.Path
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
	mux.HandleFunc("/realms", s.handleRealms)
	mux.HandleFunc("/handoff/create", s.handleHandoffCreate)
	mux.HandleFunc("/handoff/validate", s.handleHandoffValidate)
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

// listener opens the TCP listener, TLS-enabled iff both cert and key
// are configured.
func (s *Server) listener() (net.Listener, error) {
	addr := fmt.Sprintf(":%d", s.cfg.Port)
	if s.cfg.TLSCertFile != "" && s.cfg.TLSKeyFile != "" {
		cert, err := tls.LoadX509KeyPair(s.cfg.TLSCertFile, s.cfg.TLSKeyFile)
		if err != nil {
			return nil, fmt.Errorf("load TLS cert/key: %w", err)
		}
		return tls.Listen("tcp", addr, &tls.Config{MinVersion: tls.VersionTLS12, Certificates: []tls.Certificate{cert}})
	}
	return net.Listen("tcp", addr)
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

	ln, err := srv.listener()
	if err != nil {
		fmt.Fprintln(os.Stderr, "listen:", err)
		os.Exit(1)
	}
	if len(cfg.TLSCertFile) > 0 {
		fmt.Printf("authapi listening on %s (tls, db %s@%s:%d/%s)\n",
			ln.Addr().String(), cfg.AuthDB.User, cfg.AuthDB.Host, cfg.AuthDB.Port, cfg.AuthDB.Database)
	} else {
		fmt.Printf("authapi listening on %s (db %s@%s:%d/%s)\n",
			ln.Addr().String(), cfg.AuthDB.User, cfg.AuthDB.Host, cfg.AuthDB.Port, cfg.AuthDB.Database)
	}

	httpServer := &http.Server{Handler: srv.handler(), ReadHeaderTimeout: readHeaderTimeout}
	errCh := make(chan error, 1)
	go func() { errCh <- httpServer.Serve(ln) }()
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
