package main

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"strconv"
	"sync"
	"time"
)

const (
	// maxBodyBytes bounds how much request data the API accepts.
	maxBodyBytes int64 = 64 * 1024
	// authTimestampWindow is how far a request timestamp may deviate
	// from the server clock before the request is rejected.
	authTimestampWindow = 90 * time.Second
)

// Permission names an consuming service may have been granted.
// The list is closed; a permission is only added when a concrete
// consuming service demonstrably needs it (see
// docs/Auth_API_Architektur.md section 10).
const (
	permAccountAuthenticate = "account.authenticate"
	permAccountRegister     = "account.register"
	permAccountPassword     = "account.password_change"
	permAccountRecovery     = "account.recovery"
	permAccountPermissions  = "account.permissions"
	permSessionCreate       = "session.create"
	permSessionValidate     = "session.validate"
	permSessionRevoke       = "session.revoke"
	permRealmList           = "realm.list"
	permHandoffCreate       = "handoff.create"
	permHandoffValidate     = "handoff.validate"
	permWorldAuthenticate   = "world.authenticate"
	permWorldHeartbeat      = "world.heartbeat"
)

// authorize checks the service identity and signature of a request.
// Every consuming service authenticates with its OWN configured
// credentials; there is no shared secret. The service sends:
//
//	X-Andora-Service:  <service id>
//	X-Andora-Signature: hex(HMAC-SHA256(secret, payload))
//	X-Andora-Timestamp: unix seconds
//
// where
//
//	payload = METHOD \n PATH \n RAW_QUERY \n TIMESTAMP \n SHA256(BODY)
//
// On success the function returns the request body (already read, so
// the handler can re-read it via the replaced r.Body) and the
// credential record of the caller.
func (s *Server) authorize(w http.ResponseWriter, r *http.Request, requiredPerm string) ([]byte, ServiceCred, bool) {
	id := r.Header.Get("X-Andora-Service")
	if id == "" {
		writeError(w, http.StatusUnauthorized, "missing X-Andora-Service")
		return nil, ServiceCred{}, false
	}
	cred, ok := s.cfg.Services[id]
	if !ok {
		writeError(w, http.StatusUnauthorized, "unknown service")
		return nil, ServiceCred{}, false
	}
	sig := r.Header.Get("X-Andora-Signature")
	tsStr := r.Header.Get("X-Andora-Timestamp")
	ts, err := strconv.ParseInt(tsStr, 10, 64)
	if err != nil {
		writeError(w, http.StatusUnauthorized, "missing X-Andora-Timestamp")
		return nil, ServiceCred{}, false
	}
	now := time.Now().Unix()
	if diff := now - ts; diff < -int64(authTimestampWindow.Seconds()) || diff > int64(authTimestampWindow.Seconds()) {
		writeError(w, http.StatusUnauthorized, "stale X-Andora-Timestamp")
		return nil, ServiceCred{}, false
	}
	body, err := io.ReadAll(io.LimitReader(r.Body, maxBodyBytes))
	if err != nil {
		writeError(w, http.StatusBadRequest, "unreadable body")
		return nil, ServiceCred{}, false
	}
	if sig != signPayload(cred.Secret, r.Method, r.URL.Path, r.URL.RawQuery, ts, body) {
		writeError(w, http.StatusUnauthorized, "bad signature")
		return nil, ServiceCred{}, false
	}
	if !cred.hasPermission(requiredPerm) {
		writeError(w, http.StatusForbidden, "service not allowed to call this endpoint")
		return nil, ServiceCred{}, false
	}
	if ok, retry := s.rl.allow("svc:" + cred.ID + ":" + r.URL.Path); !ok {
		w.Header().Set("Retry-After", strconv.Itoa(retry))
		writeError(w, http.StatusTooManyRequests, "rate limit exceeded")
		return nil, ServiceCred{}, false
	}
	r.Body = io.NopCloser(bytes.NewReader(body))
	return body, cred, true
}

// signPayload computes the canonical HMAC-SHA256 signature the client
// must send for a request signed with a service's own secret.
func signPayload(secret, method, path, rawQuery string, ts int64, body []byte) string {
	payload := fmt.Sprintf("%s\n%s\n%s\n%d\n%s",
		method, path, rawQuery, ts, sha256Hex(body))
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write([]byte(payload))
	return hex.EncodeToString(mac.Sum(nil))
}

func sha256Hex(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

// hasPermission reports whether the service credential carries the
// given permission.
func (c ServiceCred) hasPermission(p string) bool {
	for _, x := range c.Permissions {
		if x == p {
			return true
		}
	}
	return false
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}

func newRateLimit(cfg *Config) *rateLimit {
	return &rateLimit{burst: cfg.RateLimitBurst, perMin: cfg.RateLimitPerMin, hits: map[string][]int64{}}
}

// rateLimit bounds unauthenticated and authenticated request rates.
// Before service authentication the caller is unknown, so the key is
// (remote address, path); after authentication the calling service is
// known and the key is (service id, path). Values are configured via
// Config (RateLimitBurst / RateLimitPerMin); the in-memory state is
// per-process, which is the intended deployment shape (one auth
// service, horizontally scalable later by sharding).
type rateLimit struct {
	mu        sync.Mutex
	burst     int
	perMin    int
	hits      map[string][]int64
	lastSweep int64
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

// loggingMiddleware adds a single per-request log line on the
// server side. The line contains method, path, status and the
// presenting service identity when authenticated; it never includes
// a body, signature or any secret.
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
		svc := r.Header.Get("X-Andora-Service")
		log.Printf("%s %s %d %s %v", r.Method, r.URL.Path, rec.code, svc, time.Since(start))
	}
	return http.HandlerFunc(f)
}
