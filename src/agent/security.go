package main

import (
	"crypto/hmac"
	"encoding/json"
	"log"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"
)

// writeJSON sets Content-Type and encodes v as JSON.
func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

// writeError writes a JSON error response.
func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}

// checkAuth verifies the token from header or query parameter using
// constant-time comparison. Returns true if valid.
func checkAuth(r *http.Request, token string) bool {
	presented := strings.TrimSpace(r.Header.Get("X-Andora-Token"))
	if presented == "" {
		presented = strings.TrimSpace(r.Header.Get("x-api-token"))
	}
	if presented == "" {
		presented = strings.TrimSpace(r.URL.Query().Get("token"))
	}
	if presented == "" {
		return false
	}
	return hmac.Equal([]byte(presented), []byte(token))
}

// requireAuth is a middleware wrapper that rejects unauthenticated
// requests with 401.
func requireAuth(token string, next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !checkAuth(r, token) {
			writeError(w, http.StatusUnauthorized, "unauthorized")
			return
		}
		next(w, r)
	}
}

// rateLimit bounds request rates per key. Sliding window: at most
// burst requests within one second, at most perMin within 60 seconds.
// Old entries are pruned on access so a key recovers once the window
// slides past (no permanent lockout).
type rateLimit struct {
	mu     sync.Mutex
	burst  int
	perMin int
	hits   map[string][]int64
}

func newRateLimit(burst, perMin int) *rateLimit {
	return &rateLimit{burst: burst, perMin: perMin, hits: map[string][]int64{}}
}

func (rl *rateLimit) allow(key string) (bool, int) {
	now := time.Now().Unix()
	rl.mu.Lock()
	defer rl.mu.Unlock()
	keep := rl.hits[key][:0]
	recent := 0
	withinBurst := 0
	for _, t := range rl.hits[key] {
		if t <= now-60 {
			continue // prune entries outside the minute window
		}
		keep = append(keep, t)
		recent++
		if t > now-1 {
			withinBurst++
		}
	}
	if withinBurst >= rl.burst {
		rl.hits[key] = keep
		return false, 1
	}
	if recent >= rl.perMin {
		retry := int(keep[0] + 60 - now)
		if retry < 1 {
			retry = 1
		}
		rl.hits[key] = keep
		return false, retry
	}
	rl.hits[key] = append(keep, now)
	return true, 0
}

// loggingMiddleware adds a single per-request log line. The line
// contains method, path, status and elapsed; it never includes a
// body, token or any other secret.
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
// client does not cache responses.
func noCacheHandler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate")
		w.Header().Set("Pragma", "no-cache")
		w.Header().Set("Expires", "0")
		next.ServeHTTP(w, r)
	})
}

// parseIntQuery parses a query parameter with a default and clamp.
func parseIntQuery(r *http.Request, key string, def, min, max int) int {
	s := strings.TrimSpace(r.URL.Query().Get(key))
	if s == "" {
		return def
	}
	n, err := strconv.Atoi(s)
	if err != nil {
		return def
	}
	if n < min {
		return min
	}
	if n > max {
		return max
	}
	return n
}
