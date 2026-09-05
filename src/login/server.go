package main

import (
	"bytes"
	"context"
	"crypto/tls"
	"encoding/json"
	"fmt"
	"io"
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
	// maxBodyBytes bounds how much request data the login service accepts.
	maxBodyBytes int64 = 64 * 1024
	// readHeaderTimeout bounds how long the http server waits for headers.
	readHeaderTimeout = 15 * time.Second
)

// Server is the login service: client-facing login flow, no database.
// All account state lives behind the Auth/API-Service.
type Server struct {
	cfg   *Config
	auth  *AuthAPI
	rl    *rateLimit
	start time.Time
}

func newServer(cfg *Config) *Server {
	return &Server{cfg: cfg, auth: newAuthAPI(cfg), rl: newRateLimit(cfg), start: time.Now()}
}

// handler assembles the middleware + routing chain. Tests use the same
// chain against a fake Auth-API.
func (s *Server) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", s.handleHealth)
	mux.HandleFunc("/status", s.handleStatus)
	mux.HandleFunc("/login", s.handleLogin)
	mux.HandleFunc("/logout", s.handleLogout)
	mux.HandleFunc("/realms", s.handleRealms)
	mux.HandleFunc("/handoff", s.handleHandoff)
	return loggingMiddleware(noCacheHandler(s.ratePreAuth(mux)))
}

// ratePreAuth applies the per-client rate limit (caller unknown, so the
// key is IP + path). Same shape as the Auth/API-Service.
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
	if (cfg.TLSCertFile == "") != (cfg.TLSKeyFile == "") {
		fmt.Fprintln(os.Stderr, "config: TLS requires both TLS_CERT_FILE and TLS_KEY_FILE")
		os.Exit(1)
	}
	srv := newServer(cfg)

	ln, err := srv.listener()
	if err != nil {
		fmt.Fprintln(os.Stderr, "listen:", err)
		os.Exit(1)
	}
	fmt.Printf("login listening on %s (authapi %s)\n", ln.Addr().String(), cfg.AuthAPIURL)

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
		fmt.Println("login shutting down")
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := httpServer.Shutdown(ctx); err != nil {
			fmt.Fprintln(os.Stderr, "shutdown:", err)
			os.Exit(1)
		}
	}
}

// --- shared helpers ---

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}

// readBody reads and validates a JSON request body (size-bounded).
func readBody(w http.ResponseWriter, r *http.Request) ([]byte, bool) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "use POST")
		return nil, false
	}
	body, err := io.ReadAll(io.LimitReader(r.Body, maxBodyBytes))
	if err != nil || len(body) == 0 {
		writeError(w, http.StatusBadRequest, "unreadable body")
		return nil, false
	}
	if !json.Valid(body) {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return nil, false
	}
	return body, true
}

// authUnavailable maps Auth-API failures fail-closed: the login flow
// must not continue, and no details leak to the client.
func authUnavailable(w http.ResponseWriter, err error) {
	log.Printf("authapi unavailable: %v", err)
	writeError(w, http.StatusServiceUnavailable, "auth service unavailable")
}

// rateLimit bounds unauthenticated request rates per (remote address,
// path). Same shape as the Auth/API-Service.
type rateLimit struct {
	mu        sync.Mutex
	burst     int
	perMin    int
	hits      map[string][]int64
	lastSweep int64
}

func newRateLimit(cfg *Config) *rateLimit {
	return &rateLimit{burst: cfg.RateLimitBurst, perMin: cfg.RateLimitPerMin, hits: map[string][]int64{}}
}

// allow decides whether a request for key is within the limit
// (burst + a sliding one-minute window). On rejection it returns the
// seconds until the caller may retry (Retry-After).
func (rl *rateLimit) allow(key string) (bool, int) {
	now := time.Now().Unix()
	rl.mu.Lock()
	defer rl.mu.Unlock()
	if now-rl.lastSweep >= 120 {
		for k, t := range rl.hits {
			if len(t) == 0 {
				delete(rl.hits, k)
				continue
			}
			if t[0] < now-180 {
				delete(rl.hits, k)
			} else {
				rl.hits[k] = t
			}
		}
		rl.lastSweep = now
	}
	ts := rl.hits[key]
	var retry int
	if len(ts) >= rl.burst {
		oldest := ts[0]
		if len(ts) >= rl.perMin {
			cutoff := now - 60
			retry = int(oldest + 1 - cutoff)
			if retry < 1 {
				retry = 1
			}
		} else {
			retry = int(oldest + 1 - now)
			if retry < 1 {
				retry = 1
			}
		}
		return false, retry
	}
	rl.hits[key] = append(ts, now)
	return true, 0
}

// remoteAddr extracts the presenting client IP (X-Forwarded-For is
// deliberately NOT trusted here: a spoofed forwarder would let one
// IP abuse the budget of another).
func remoteAddr(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

// loggingMiddleware adds a single per-request log line. The line
// contains method, path, status and duration; it never includes a
// body, password or any secret.
func loggingMiddleware(next http.Handler) http.Handler {
	type statusRecorder struct {
		http.ResponseWriter
		code int
	}
	f := func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		rec := &statusRecorder{w, 0}
		next.ServeHTTP(rec, r)
		if rec.code == 0 {
			rec.code = http.StatusOK
		}
		log.Printf("%s %s %d %v", r.Method, r.URL.Path, rec.code, time.Since(start))
	}
	return http.HandlerFunc(f)
}

// noCacheHandler sets no-store and no-cache headers so a proxy or
// client does not cache login responses.
func noCacheHandler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate")
		w.Header().Set("Pragma", "no-cache")
		w.Header().Set("Expires", "0")
		next.ServeHTTP(w, r)
	})
}

// decodeJSON is a tiny helper for request parsing in handlers.
func decodeJSON(body []byte, v any) bool {
	dec := json.NewDecoder(bytes.NewReader(body))
	if err := dec.Decode(v); err != nil {
		return false
	}
	return true
}
