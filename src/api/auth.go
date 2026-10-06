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
	"net/http"
	"strconv"
	"strings"
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
	permAccountTwoFactor    = "account.two_factor"
	permDeviceList          = "device.list"
	permDeviceRevoke        = "device.revoke"
	permSecurityEvents      = "security.events"
	permSessionCreate       = "session.create"
	permSessionValidate     = "session.validate"
	permSessionRevoke       = "session.revoke"
	permRealmList           = "realm.list"
	permHandoffCreate       = "handoff.create"
	permHandoffValidate     = "handoff.validate"
	// LEGACY (kein separater Worldserver mehr, Kette Auth/API → Login → Realm):
	// world.authenticate/world.heartbeat werden von keinem Dienst mehr
	// verwendet. Endpunkte bleiben aus Kompatibilität bestehen, es darf
	// nichts Neues darauf aufgebaut werden.
	permWorldAuthenticate = "world.authenticate"
	permWorldHeartbeat    = "world.heartbeat"
	permParentalManage    = "parental.manage"
	permParentalPin       = "parental.pin"
	permParentalStatus    = "parental.status"
	permParentalNotify    = "parental.notifications"
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

// accessLogPathMax is how many characters of a request-determined field may
// appear in one access-log line before it is visibly truncated. It is a
// bound of the LOG representation only: it changes neither the request nor
// the answer. Every registered route is far shorter, so no legitimate path
// is ever shortened.
const accessLogPathMax = 64

// accessLogField renders a request-determined value as exactly ONE bounded
// line segment. The value reaches us percent-decoded (r.URL.Path), so it may
// contain control characters and spaces; both are escaped here. Escaping the
// space is what keeps the space-separated fields of the access log
// unambiguous: a value can therefore never introduce an additional field.
// A trailing "~" marks a value that was shortened.
//
// This is a display boundary. It is NOT a payload rule: it does not accept
// or reject any request and does not alter any handler.
func accessLogField(raw string) string {
	var b strings.Builder
	b.Grow(len(raw))
	count := 0
	for _, r := range raw {
		if count == accessLogPathMax {
			b.WriteByte('~')
			break
		}
		switch {
		case r == '\\':
			b.WriteString(`\\`)
		case r == ' ':
			b.WriteString(`\x20`)
		case r < 0x20 || r == 0x7f:
			const hex = "0123456789abcdef"
			b.WriteString(`\x`)
			b.WriteByte(hex[byte(r)>>4])
			b.WriteByte(hex[byte(r)&0x0f])
		default:
			b.WriteRune(r)
		}
		count++
	}
	return b.String()
}

// loggingMiddleware adds a single per-request log line on the
// server side. The line contains method, path, status and duration.
//
// It deliberately contains NO header value: the presenting service
// identity arrives in the unauthenticated X-Andora-Service header and is
// therefore caller-controlled on every request, including rejected ones.
// It is not logged at all, neither raw nor validated. Passing on the
// AUTHORIZED identity would require threading the credential out of
// authorize(); that is a separate decision and not needed here.
//
// Body, query values, signature and every other header stay out too.
// The path is the only remaining request-determined field and it is
// rendered through accessLogField (single line, escaped, bounded).
// statusRecorder merkt sich den WIRKSAMEN finalen Antwortstatus einer
// Anfrage.
//
// Vorher existierte kein WriteHeader-Override: der eingebettete
// ResponseWriter erhielt den Aufruf, das Statusfeld blieb 0 und der Fallback
// trug immer 200 ein — jede Ablehnung war im Access-Log ein Erfolg.
//
// Die Semantik folgt der net/http-Vertragslage (ResponseWriter.WriteHeader
// und response.WriteHeader in net/http/server.go):
//
//   - WriteHeader(c) merkt c als final, solange noch keiner gesetzt ist.
//   - 1xx (100..199, ohne 101) sind informative Antworten. net/http sendet
//     sie sofort, setzt `wroteHeader` ausdrücklich NICHT und lässt `status`
//     unverändert; eine beliebige Anzahl darf folgen. Sie sind daher KEIN
//     finaler Status: sie werden durchgereicht, aber nicht gemerkt.
//   - 101 (Switching Protocols) nimmt in net/http den nicht-informativen
//     Pfad und ist damit final.
//   - Ein späteres WriteHeader überschreibt einen bereits wirksamen finalen
//     Status nicht; net/http verwirft solche Aufrufe als "superfluous". Der
//     erste finale Status bleibt.
//   - Write ohne vorheriges WriteHeader löst implizit 200 aus; der
//     eigentliche Writer löst das ebenfalls auf, hier wird es nur
//     mitgebucht, damit der Logwert stimmt.
//   - Schreibt der Handler überhaupt nicht, gilt 200.
type statusRecorder struct {
	http.ResponseWriter
	code      int
	finalized bool
}

// writeHeader merkt einen finalen Status. Ist bereits ein finaler Status
// wirksam, wird der Aufruf verworfen und der bestehende beibehalten.
// Informative 1xx-Antworten werden durchgereicht, aber nicht gemerkt.
func (sr *statusRecorder) writeHeader(code int) {
	if code >= 100 && code <= 199 && code != http.StatusSwitchingProtocols {
		sr.ResponseWriter.WriteHeader(code)
		return
	}
	if sr.finalized {
		return
	}
	sr.code = code
	sr.finalized = true
	sr.ResponseWriter.WriteHeader(code)
}

// WriteHeader reicht den Status durch und merkt den finalen Status.
func (sr *statusRecorder) WriteHeader(code int) {
	sr.writeHeader(code)
}

// Write bucht den impliziten Status 200, falls noch keiner wirksam ist, und
// reicht die Daten durch.
func (sr *statusRecorder) Write(b []byte) (int, error) {
	if !sr.finalized {
		sr.writeHeader(http.StatusOK)
	}
	return sr.ResponseWriter.Write(b)
}

// statusOrOK liefert den wirksamen finalen Status; ohne jeden Schreibvorgang
// gilt 200.
func (sr *statusRecorder) statusOrOK() int {
	if !sr.finalized {
		return http.StatusOK
	}
	return sr.code
}

func loggingMiddleware(next http.Handler) http.Handler {
	f := func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		rec := &statusRecorder{ResponseWriter: w}
		next.ServeHTTP(rec, r)
		log.Printf("%s %s %d %v", r.Method, accessLogField(r.URL.Path), rec.statusOrOK(), time.Since(start))
	}
	return http.HandlerFunc(f)
}
