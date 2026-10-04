package main

import (
	"context"
	"crypto/rand"
	"database/sql"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"
)

// --- shared validation ---

func validUsername(u string) bool {
	if len(u) < 3 || len(u) > 32 {
		return false
	}
	for _, c := range u {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_') {
			return false
		}
	}
	return true
}

// validEmail is a conservative e-mail check (no regexp dependency):
// one '@', at least one '.' in the domain, allowed chars only.
func validEmail(e string) bool {
	if len(e) < 6 || len(e) > 254 {
		return false
	}
	at := strings.Index(e, "@")
	if at <= 0 || at == len(e)-1 {
		return false
	}
	domain := e[at+1:]
	if strings.Index(domain, ".") < 0 {
		return false
	}
	for _, c := range e {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' ||
			c == '.' || c == '_' || c == '%' || c == '+' || c == '-' || c == '@') {
			return false
		}
	}
	return true
}

// validPassword: 8..72 chars (argon2 has no hard limit, 72 is the
// practical cap we enforce).
func validPassword(p string) bool {
	return len(p) >= 8 && len(p) <= 72
}

func dbError(w http.ResponseWriter, err error, fallback string) {
	if err == sql.ErrNoRows {
		writeError(w, http.StatusNotFound, "not found")
		return
	}
	writeError(w, http.StatusInternalServerError, fallback)
}

// --- session monitoring (P-33) ---
//
// /status is OPEN (no service signature), so this block publishes
// aggregates only: counts, one age, one delta, one metadata estimate set
// and durations. No token, token hash, session id, account id, username,
// single row or raw database error ever enters the response; driver
// messages go to the log only.
//
// Collection is DEMAND-DRIVEN by /status and never blocks a response:
// the handler renders the published snapshot first and only then admits
// at most one worker goroutine. There is no ticker and no history — one
// immutable snapshot, replaced atomically. Monitoring is read-only: it
// deletes nothing and revokes nothing.

// sessionStatsState classifies the cached inventory for /status.
type sessionStatsState string

const (
	// sessionStatsUnknown: no attempt has produced an inventory yet.
	sessionStatsUnknown sessionStatsState = "unknown"
	// sessionStatsDisabled: collection is switched off by configuration.
	sessionStatsDisabled sessionStatsState = "disabled"
	// sessionStatsRunning: an attempt is in flight and no successful
	// inventory exists yet, so nothing can be shown.
	sessionStatsRunning sessionStatsState = "running"
	// sessionStatsOK: the newest attempt succeeded and the stand is fresh.
	sessionStatsOK sessionStatsState = "ok"
	// sessionStatsStale: the last successful stand is older than twice
	// the minimum interval. The values are still shown, but as stale.
	sessionStatsStale sessionStatsState = "stale"
	// sessionStatsError: the newest attempt failed. The previous
	// successful stand is kept; if there is none, nothing is invented.
	sessionStatsError sessionStatsState = "error"
)

// Metadata states, kept separate from the inventory state because a
// metadata failure must never invalidate successfully published counters.
const (
	sessionSizeUnknown  = "unknown"
	sessionSizeOK       = "ok"
	sessionSizeNoAccess = "no_access"
	sessionSizeError    = "error"
)

// sessionStatsSnapshot is the published state of the monitoring. It is
// immutable once published: every change creates a new value, so a reader
// always sees a consistent generation.
type sessionStatsSnapshot struct {
	state         sessionStatsState
	inFlight      bool
	hasAttempt    bool
	collectedAt   time.Time
	lastAttemptAt time.Time
	// lastAttemptOK is the explicit outcome of the newest attempt. It is
	// stored, never derived from a timestamp comparison.
	lastAttemptOK bool
	attempts      uint64

	// Inventory facts describe the last SUCCESSFUL collection and are
	// carried forward unchanged by a failed attempt.
	hasInventory bool
	inventory    SessionInventory
	delta        *int64
	deltaWindow  *float64

	// Metadata facts are independent of the inventory and carry their
	// own timestamp, so a metadata stand never carries the timestamp of
	// newer counters.
	size      SessionTableSize
	sizeState string
	sizeAt    time.Time
	hasSize   bool

	// Batch lookup timing comes from the store, not from this collection.
	batchLast       time.Duration
	batchMax        time.Duration
	batchMeasuredAt time.Time
	hasBatch        bool
}

// sessionStatsLimits returns the effective limits. Non-positive
// configuration values fall back to the documented defaults.
func (s *Server) sessionStatsLimits() (minInterval, timeout time.Duration) {
	minInterval = time.Duration(defaultSessionStatsMinIntervalSecs) * time.Second
	timeout = time.Duration(defaultSessionStatsTimeoutMS) * time.Millisecond
	if s.cfg == nil {
		return minInterval, timeout
	}
	if s.cfg.SessionStatsMinIntervalSecs > 0 {
		minInterval = time.Duration(s.cfg.SessionStatsMinIntervalSecs) * time.Second
	}
	if s.cfg.SessionStatsTimeoutMS > 0 {
		timeout = time.Duration(s.cfg.SessionStatsTimeoutMS) * time.Millisecond
	}
	return minInterval, timeout
}

// sessionStatsEnabled reports whether a collection attempt may run.
func (s *Server) sessionStatsEnabled() bool {
	return s.cfg != nil && s.cfg.SessionStatsEnabled
}

// admitSessionStats performs the whole admission decision under one
// lock: closing state, in-flight state, minimum distance, attempt counter
// and the completion bookkeeping. It returns true only for the single
// caller that may start a worker. cancel is stored so close() can abort the
// attempt it just admitted.
func (s *Server) admitSessionStats(now time.Time, minInterval time.Duration, cancel context.CancelFunc) bool {
	s.sessionStatsMu.Lock()
	defer s.sessionStatsMu.Unlock()
	// Once close() has begun, no further attempt is admitted. This and the
	// counter below live under the same lock, so close() cannot observe an
	// idle moment and then admit a new worker behind its back.
	if s.sessionStatsClosing {
		return false
	}
	if s.sessionStatsInFlight {
		return false
	}
	if !s.sessionStatsNextAllowed.IsZero() && now.Before(s.sessionStatsNextAllowed) {
		return false
	}
	s.sessionStatsInFlight = true
	// The minimum distance is counted from the START of the attempt, so
	// it also throttles retries after a failed or timed-out attempt.
	s.sessionStatsNextAllowed = now.Add(minInterval)
	s.sessionStatsAttempts++
	s.sessionStatsRunning++
	if s.sessionStatsDone == nil {
		s.sessionStatsDone = make(chan struct{})
	}
	s.sessionStatsCancel = cancel
	return true
}

// triggerSessionStats starts at most one bounded collection worker. It
// never waits and never blocks the caller.
func (s *Server) triggerSessionStats() {
	if !s.sessionStatsEnabled() {
		return
	}
	_, timeout := s.sessionStatsLimits()
	// The attempt context is created before the admission decision so that
	// close() can cancel the very attempt it admitted. A cancelled HTTP
	// request never aborts a collection: the context is rooted in Background.
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	if !s.admitSessionStats(time.Now(), mustSessionStatsMinInterval(s), cancel) {
		// Not admitted: release the context resources right away.
		cancel()
		return
	}
	go s.collectSessionStats(ctx)
}

func mustSessionStatsMinInterval(s *Server) time.Duration {
	minInterval, _ := s.sessionStatsLimits()
	return minInterval
}

// collectSessionStats is the demand-triggered worker goroutine. It is
// time-boxed by the context created in triggerSessionStats, uses that
// context (a cancelled HTTP request must not abort a collection) and
// releases the in-flight state on every exit path.
func (s *Server) collectSessionStats(ctx context.Context) {
	defer s.finishSessionStatsAttempt()

	startedAt := time.Now()
	inv, invErr := s.store.SessionInventory(ctx)
	size, sizeErr := s.store.SessionTableSize(ctx)
	s.publishSessionStats(startedAt, inv, invErr, size, sizeErr)
}

// finishSessionStatsAttempt releases the in-flight state, frees the
// attempt's context and signals completion. In-flight MUST be released
// here and nowhere else, so every exit path of the worker is covered. The
// completion channel is closed by the LAST returning attempt, which is what
// close() waits for.
func (s *Server) finishSessionStatsAttempt() {
	s.sessionStatsMu.Lock()
	s.sessionStatsInFlight = false
	if s.sessionStatsCancel != nil {
		// Idempotent: releases the attempt's timer and cancels anything the
		// attempt may still have pending.
		s.sessionStatsCancel()
		s.sessionStatsCancel = nil
	}
	if s.sessionStatsRunning > 0 {
		s.sessionStatsRunning--
	}
	if s.sessionStatsRunning == 0 && s.sessionStatsDone != nil {
		close(s.sessionStatsDone)
		s.sessionStatsDone = nil
	}
	s.sessionStatsMu.Unlock()
	if s.sessionStatsSignal != nil {
		select {
		case s.sessionStatsSignal <- struct{}{}:
		default:
		}
	}
}

// publishSessionStats builds the next immutable snapshot and swaps it in.
// A failed attempt keeps the previous successful stand (counters, age and
// delta) and only updates the attempt facts; it never produces a new
// comparison base for the growth delta.
func (s *Server) publishSessionStats(startedAt time.Time, inv SessionInventory, invErr error, size SessionTableSize, sizeErr error) {
	now := time.Now()
	minInterval, _ := s.sessionStatsLimits()

	s.sessionStatsMu.Lock()
	prev := s.sessionStatsPub
	next := &sessionStatsSnapshot{
		lastAttemptAt: startedAt,
		lastAttemptOK: invErr == nil,
		hasAttempt:    true,
	}
	if prev != nil {
		next.attempts = prev.attempts
		// Carry the successful stand forward.
		next.hasInventory = prev.hasInventory
		next.inventory = prev.inventory
		next.delta = prev.delta
		next.deltaWindow = prev.deltaWindow
		next.collectedAt = prev.collectedAt
		next.size = prev.size
		next.sizeState = prev.sizeState
		next.sizeAt = prev.sizeAt
		next.hasSize = prev.hasSize
		next.batchLast = prev.batchLast
		next.batchMax = prev.batchMax
		next.batchMeasuredAt = prev.batchMeasuredAt
		next.hasBatch = prev.hasBatch
	}
	next.attempts++

	if invErr == nil {
		next.hasInventory = true
		next.inventory = inv
		next.collectedAt = now
		// Growth against the previous SUCCESSFUL inventory. A first
		// collection or a non-positive window yields no delta at all
		// instead of an invented rate.
		if prev != nil && prev.hasInventory {
			window := now.Sub(prev.collectedAt)
			if window > 0 {
				delta := inv.Total - prev.inventory.Total
				w := window.Seconds()
				next.delta = &delta
				next.deltaWindow = &w
			}
		}
	}

	// Metadata is independent of the counters.
	switch {
	case errors.Is(sizeErr, errSessionSizeNoAccess):
		next.sizeState = sessionSizeNoAccess
		next.size = SessionTableSize{}
		next.hasSize = false
		next.sizeAt = time.Time{}
	case sizeErr != nil:
		next.sizeState = sessionSizeError
		next.size = SessionTableSize{}
		next.hasSize = false
		next.sizeAt = time.Time{}
	case size.TableRows == nil && size.DataBytes == nil && size.IndexBytes == nil:
		// No visible metadata row: unknown, not zero.
		next.sizeState = sessionSizeUnknown
		next.size = SessionTableSize{}
		next.hasSize = false
		next.sizeAt = time.Time{}
	default:
		next.sizeState = sessionSizeOK
		next.size = size
		next.hasSize = true
		next.sizeAt = now
	}

	if t, ok := s.store.(batchLookupTimer); ok {
		if last, max, measuredAt, measured := t.BatchLookupTiming(); measured {
			next.hasBatch = true
			next.batchLast = last
			next.batchMax = max
			next.batchMeasuredAt = measuredAt
		}
	}

	next.state = resolveSessionStatsState(next, now, minInterval, s.sessionStatsEnabled())
	s.sessionStatsPub = next
	s.sessionStatsMu.Unlock()

	logSessionStats(next, invErr, sizeErr, now.Sub(startedAt))
}

// resolveSessionStatsState derives the reported state. It is a pure
// function of the snapshot so that the rendering cannot invent anything.
// sessionStatsStaleAfter is the ONE definition of the staleness threshold:
// the age from which a published stand counts as stale. It is twice the
// effective minimum distance, so a stand stays fresh across one skipped
// attempt. The renderer publishes exactly this value, which is why output
// and decision cannot drift apart.
//
// The threshold is a pure DISPLAY convention of this read-only view. It is
// NOT a retention period, NOT an action threshold and NOT a deletion signal.
func sessionStatsStaleAfter(minInterval time.Duration) time.Duration {
	return 2 * minInterval
}

func resolveSessionStatsState(sn *sessionStatsSnapshot, now time.Time, minInterval time.Duration, enabled bool) sessionStatsState {
	if !enabled {
		return sessionStatsDisabled
	}
	if !sn.hasInventory {
		if sn.inFlight {
			return sessionStatsRunning
		}
		if sn.hasAttempt && !sn.lastAttemptOK {
			return sessionStatsError
		}
		return sessionStatsUnknown
	}
	if sn.hasAttempt && !sn.lastAttemptOK {
		return sessionStatsError
	}
	if sn.inFlight {
		// The existing stand stays usable while a refresh runs.
		if now.Sub(sn.collectedAt) > sessionStatsStaleAfter(minInterval) {
			return sessionStatsStale
		}
		return sessionStatsOK
	}
	if now.Sub(sn.collectedAt) > sessionStatsStaleAfter(minInterval) {
		return sessionStatsStale
	}
	return sessionStatsOK
}

// logSessionStats writes exactly one aggregated line per attempt for
// later external evaluation. It contains no error text and no
// identifying data; the repository itself performs no long-term
// monitoring and no aggregation.
func logSessionStats(sn *sessionStatsSnapshot, invErr, sizeErr error, elapsed time.Duration) {
	result := "ok"
	stage := "none"
	switch {
	case invErr != nil && sizeErr != nil:
		result, stage = "error", "inventory+metadata"
	case invErr != nil:
		result, stage = "error", "inventory"
	case sizeErr != nil:
		result, stage = "partial", "metadata"
	}
	total, active, expired, revoked := "null", "null", "null", "null"
	age := "null"
	partition := "null"
	queryMS := "null"
	if sn.hasInventory {
		total = strconv.FormatInt(sn.inventory.Total, 10)
		active = strconv.FormatInt(sn.inventory.Active, 10)
		expired = strconv.FormatInt(sn.inventory.ExpiredNotRevoked, 10)
		revoked = strconv.FormatInt(sn.inventory.Revoked, 10)
		partition = strconv.FormatBool(sn.inventory.PartitionOK)
		queryMS = strconv.FormatFloat(sn.inventory.QueryMS, 'f', 3, 64)
		if sn.inventory.OldestRowAgeDays != nil {
			age = strconv.FormatInt(*sn.inventory.OldestRowAgeDays, 10)
		}
	}
	delta, window := "null", "null"
	if sn.delta != nil && sn.deltaWindow != nil {
		delta = strconv.FormatInt(*sn.delta, 10)
		window = strconv.FormatFloat(*sn.deltaWindow, 'f', 3, 64)
	}
	rowsEst, dataEst, indexEst := "null", "null", "null"
	if sn.hasSize {
		if sn.size.TableRows != nil {
			rowsEst = strconv.FormatInt(*sn.size.TableRows, 10)
		}
		if sn.size.DataBytes != nil {
			dataEst = strconv.FormatInt(*sn.size.DataBytes, 10)
		}
		if sn.size.IndexBytes != nil {
			indexEst = strconv.FormatInt(*sn.size.IndexBytes, 10)
		}
	}
	batchLast, batchMax, batchAt := "null", "null", "null"
	if sn.hasBatch {
		batchLast = strconv.FormatFloat(sn.batchLast.Seconds()*1000, 'f', 3, 64)
		batchMax = strconv.FormatFloat(sn.batchMax.Seconds()*1000, 'f', 3, 64)
		batchAt = sn.batchMeasuredAt.UTC().Format(time.RFC3339)
	}
	log.Printf("session_stats result=%s stage=%s attempts=%d state=%s total=%s active=%s "+
		"expired_not_revoked=%s revoked=%s partition_ok=%s oldest_row_age_days=%s "+
		"aggregate_query_ms=%s total_delta=%s delta_window_seconds=%s table_rows_estimate=%s "+
		"data_bytes_estimate=%s index_bytes_estimate=%s size_state=%s "+
		"session_status_batch_last_ms=%s session_status_batch_max_ms=%s batch_measured_at=%s "+
		"attempt_ms=%s",
		result, stage, sn.attempts, sn.state, total, active, expired, revoked, partition, age,
		queryMS, delta, window, rowsEst, dataEst, indexEst, sn.sizeState,
		batchLast, batchMax, batchAt, strconv.FormatFloat(elapsed.Seconds()*1000, 'f', 3, 64))
}

// statusSessionsResponse is the additive /status block. Every counter is
// a pointer: nil means "unknown", which is never confused with 0.
type statusSessionsResponse struct {
	State              string `json:"state"`
	Enabled            bool   `json:"enabled"`
	InFlight           bool   `json:"in_flight"`
	MinIntervalSeconds int    `json:"min_interval_seconds"`
	Attempts           uint64 `json:"attempts"`

	CollectedAt   *string `json:"collected_at"`
	LastAttemptAt *string `json:"last_attempt_at"`
	// LastAttemptOK is the stored outcome of the newest attempt.
	LastAttemptOK *bool    `json:"last_attempt_ok"`
	AgeSeconds    *float64 `json:"age_seconds"`
	// StaleAfterSeconds is the age from which the stand counts as stale.
	StaleAfterSeconds int `json:"stale_after_seconds"`

	Total             *int64 `json:"total"`
	Active            *int64 `json:"active"`
	ExpiredNotRevoked *int64 `json:"expired_not_revoked"`
	Revoked           *int64 `json:"revoked"`
	PartitionOK       *bool  `json:"partition_ok"`
	OldestRowAgeDays  *int64 `json:"oldest_row_age_days"`

	TotalDelta         *int64   `json:"total_delta"`
	DeltaWindowSeconds *float64 `json:"delta_window_seconds"`

	TableRowsEstimate  *int64  `json:"table_rows_estimate"`
	DataBytesEstimate  *int64  `json:"data_bytes_estimate"`
	IndexBytesEstimate *int64  `json:"index_bytes_estimate"`
	SizeState          string  `json:"size_state"`
	SizeMeasuredAt     *string `json:"size_measured_at"`

	AggregateQueryMs *float64 `json:"aggregate_query_ms"`
	// The next three values describe the PRODUCTIVE batch session-status
	// DB lookup, not the HTTP latency of an endpoint.
	SessionStatusBatchLastMs *float64 `json:"session_status_batch_last_ms"`
	SessionStatusBatchMaxMs  *float64 `json:"session_status_batch_max_ms"`
	// SessionStatusBatchMaxScope states that the maximum is per process
	// since start, not a percentile and not a time window.
	SessionStatusBatchMaxScope   string  `json:"session_status_batch_max_scope"`
	SessionStatusBatchMeasuredAt *string `json:"session_status_batch_measured_at"`
}

// renderSessionStats turns a published snapshot into the response block.
// It reads one immutable value and never waits for the database.
func (s *Server) renderSessionStats() statusSessionsResponse {
	minInterval, _ := s.sessionStatsLimits()
	enabled := s.sessionStatsEnabled()
	now := time.Now()

	s.sessionStatsMu.Lock()
	sn := s.sessionStatsPub
	// Every mutable monitoring field is copied out under its lock. After
	// Unlock only these copies are read; no field of the Server is touched
	// again, which also covers the path before the first publication where
	// there is no snapshot to copy from. inFlight and attempts are the two
	// VOLATILE facts; everything else comes from the published snapshot.
	inFlight := s.sessionStatsInFlight
	attempts := s.sessionStatsAttempts
	if sn != nil && sn.inFlight != inFlight {
		// Refresh the volatile in-flight flag on a copy: the published
		// value itself is never modified.
		cp := *sn
		cp.inFlight = inFlight
		sn = &cp
	}
	s.sessionStatsMu.Unlock()

	out := statusSessionsResponse{
		State:              string(sessionStatsUnknown),
		Enabled:            enabled,
		MinIntervalSeconds: int(minInterval / time.Second),
		// Published from the SAME definition the stale decision uses, so
		// output and decision cannot drift apart. Unit: seconds. It is a
		// pure display convention of this read-only view: NOT a retention
		// period and NOT a deletion signal.
		StaleAfterSeconds:          int(sessionStatsStaleAfter(minInterval) / time.Second),
		SessionStatusBatchMaxScope: "process_since_start",
		SizeState:                  sessionSizeUnknown,
	}
	if !enabled {
		out.State = string(sessionStatsDisabled)
		return out
	}
	if sn == nil {
		// No snapshot yet: evaluate the locked copies, not empty values, so
		// a running first collection is reported as running and the attempt
		// counter is not lost.
		probe := &sessionStatsSnapshot{inFlight: inFlight}
		out.State = string(resolveSessionStatsState(probe, now, minInterval, enabled))
		out.InFlight = inFlight
		out.Attempts = attempts
		return out
	}
	out.State = string(resolveSessionStatsState(sn, now, minInterval, enabled))
	out.InFlight = sn.inFlight
	out.Attempts = attempts
	if sn.hasAttempt {
		out.LastAttemptAt = timePtrString(sn.lastAttemptAt)
		ok := sn.lastAttemptOK
		out.LastAttemptOK = &ok
	}
	if sn.hasInventory {
		out.CollectedAt = timePtrString(sn.collectedAt)
		age := now.Sub(sn.collectedAt).Seconds()
		out.AgeSeconds = &age
		out.Total = int64Ptr(sn.inventory.Total)
		out.Active = int64Ptr(sn.inventory.Active)
		out.ExpiredNotRevoked = int64Ptr(sn.inventory.ExpiredNotRevoked)
		out.Revoked = int64Ptr(sn.inventory.Revoked)
		partition := sn.inventory.PartitionOK
		out.PartitionOK = &partition
		out.OldestRowAgeDays = sn.inventory.OldestRowAgeDays
		out.AggregateQueryMs = float64Ptr(sn.inventory.QueryMS)
	}
	if sn.delta != nil && sn.deltaWindow != nil {
		out.TotalDelta = sn.delta
		out.DeltaWindowSeconds = sn.deltaWindow
	}
	out.SizeState = sn.sizeState
	if sn.hasSize {
		out.SizeMeasuredAt = timePtrString(sn.sizeAt)
		out.TableRowsEstimate = sn.size.TableRows
		out.DataBytesEstimate = sn.size.DataBytes
		out.IndexBytesEstimate = sn.size.IndexBytes
	}
	if sn.hasBatch {
		out.SessionStatusBatchLastMs = float64Ptr(sn.batchLast.Seconds() * 1000)
		out.SessionStatusBatchMaxMs = float64Ptr(sn.batchMax.Seconds() * 1000)
		out.SessionStatusBatchMeasuredAt = timePtrString(sn.batchMeasuredAt)
	}
	return out
}

func timePtrString(t time.Time) *string {
	if t.IsZero() {
		return nil
	}
	s := t.UTC().Format(time.RFC3339)
	return &s
}

func int64Ptr(v int64) *int64 { return &v }

func float64Ptr(v float64) *float64 { return &v }

// --- /status ---

type statusResponse struct {
	Status   string                 `json:"status"`
	Uptime   string                 `json:"uptime"`
	Sessions statusSessionsResponse `json:"sessions"`
}

// handleStatus is public (no service signature): liveness + uptime +
// the read-only session monitoring block (P-33). The existing liveness
// behaviour is unchanged; the session block is rendered from the cache and
// never waits for the database. A collection is only ADMITTED after the
// response has been rendered, so the first call can still report
// "unknown" instead of waiting for a first collection.
func (s *Server) handleStatus(w http.ResponseWriter, r *http.Request) {
	body := statusResponse{
		Status:   "ok",
		Uptime:   time.Since(s.start).Round(time.Second).String(),
		Sessions: s.renderSessionStats(),
	}
	writeJSON(w, http.StatusOK, body)
	s.triggerSessionStats()
}

// --- account endpoints ---

type registerRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
	Email    string `json:"email"`
}

type registerResponse struct {
	AccountID int `json:"account_id"`
}

// handleAccountRegister creates a new account. The fresh account is
// banned for NewAccountBan (operator grace period) and the e-mail is
// stored only encrypted plus its non-reversible lookup hash.
func (s *Server) handleAccountRegister(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountRegister)
	if !ok {
		return
	}
	var req registerRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if !validUsername(req.Username) {
		writeError(w, http.StatusBadRequest, "username must be 3-32 chars [A-Za-z0-9_]")
		return
	}
	if !validPassword(req.Password) {
		writeError(w, http.StatusBadRequest, "password must be 8-72 chars")
		return
	}
	if !validEmail(req.Email) {
		writeError(w, http.StatusBadRequest, "invalid e-mail")
		return
	}
	if _, err := s.store.FetchAccount(context.Background(), "username", req.Username); err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	lookupRaw := hashLookupEmailRaw(req.Email)
	if _, err := s.store.FetchAccount(context.Background(), "email_lookup_hash", lookupRaw); err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	hash, err := HashPasswordSalt(req.Password, s.cfg.ArgonTime, s.cfg.ArgonMemory, s.cfg.ArgonThreads)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "password hash failed")
		return
	}
	emailEnc, err := encryptEmail(s.cfg.EncryptionKey, req.Email)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "email encryption failed")
		return
	}
	id, err := s.store.RegisterAccount(context.Background(), req.Username, hash, emailEnc, lookupRaw, s.cfg.NewAccountBan)
	if err != nil {
		if isDuplicateKey(err) {
			writeError(w, http.StatusConflict, "username or e-mail already registered")
			return
		}
		dbError(w, err, "register failed")
		return
	}
	writeJSON(w, http.StatusCreated, registerResponse{AccountID: id})
}

// HashPasswordSalt creates an Argon2id hash with a fresh random salt.
// Exposed for migration / re-hash tooling.
func HashPasswordSalt(password string, time_, memory uint32, threads uint8) (string, error) {
	salt := make([]byte, 16)
	if _, err := rand.Read(salt); err != nil {
		return "", err
	}
	return HashPassword(password, string(salt), time_, memory, threads), nil
}

type passwordChangeRequest struct {
	AccountID   int    `json:"account_id"`
	OldPassword string `json:"old_password"`
	NewPassword string `json:"new_password"`
}

// handlePasswordChange swaps the password after verifying the old one and
// revokes ALL sessions + trusted-device tokens of the account in the same
// transaction (security event password_changed).
func (s *Server) handlePasswordChange(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountPassword)
	if !ok {
		return
	}
	var req passwordChangeRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if !validPassword(req.NewPassword) {
		writeError(w, http.StatusBadRequest, "new password must be 8-72 chars")
		return
	}
	acc, err := s.store.FetchAccountByID(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	if !VerifyPassword(acc.PasswordHash, req.OldPassword) {
		writeError(w, http.StatusUnauthorized, "old password does not match")
		return
	}
	hash, err := HashPasswordSalt(req.NewPassword, s.cfg.ArgonTime, s.cfg.ArgonMemory, s.cfg.ArgonThreads)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "password hash failed")
		return
	}
	if err := s.store.ChangePasswordRevokeAll(context.Background(), req.AccountID, hash); err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusNotFound, "account not found")
			return
		}
		dbError(w, err, "password change failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"changed": true})
}

type recoveryRequest struct {
	AccountID int `json:"account_id"`
}

type recoveryResponse struct {
	RecoveryToken string `json:"recovery_token"`
	ExpiresAt     string `json:"expires_at"`
}

// handleRecoveryRequest creates a single-use recovery token. Delivery
// of the token to the account owner is out-of-band (e-mail sending is
// deliberately not part of this service; see README).
func (s *Server) handleRecoveryRequest(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountRecovery)
	if !ok {
		return
	}
	var req recoveryRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	acc, err := s.store.FetchAccountByID(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	raw, sess, err := s.store.CreateRecoveryToken(context.Background(), req.AccountID, s.cfg.RecoveryTTL)
	if err != nil {
		dbError(w, err, "recovery request failed")
		return
	}
	writeJSON(w, http.StatusOK, recoveryResponse{RecoveryToken: raw, ExpiresAt: sess.ExpiresAt.Format(time.RFC3339)})
}

type recoveryConfirmRequest struct {
	RecoveryToken string `json:"recovery_token"`
	NewPassword   string `json:"new_password"`
}

// handleRecoveryConfirm sets a new password and clears the ban,
// consuming the single-use token atomically.
func (s *Server) handleRecoveryConfirm(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountRecovery)
	if !ok {
		return
	}
	var req recoveryConfirmRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if !validPassword(req.NewPassword) {
		writeError(w, http.StatusBadRequest, "new password must be 8-72 chars")
		return
	}
	rec, err := s.store.ValidateRecovery(context.Background(), req.RecoveryToken)
	if err != nil {
		dbError(w, err, "recovery lookup failed")
		return
	}
	if rec == nil {
		writeError(w, http.StatusUnauthorized, "recovery token invalid or already used")
		return
	}
	hash, err := HashPasswordSalt(req.NewPassword, s.cfg.ArgonTime, s.cfg.ArgonMemory, s.cfg.ArgonThreads)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "password hash failed")
		return
	}
	if err := s.store.RecoverPassword(context.Background(), rec.AccountID, hash, req.RecoveryToken); err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusUnauthorized, "recovery token invalid or already used")
			return
		}
		dbError(w, err, "recovery confirm failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"recovered": true})
}

// --- account permissions ---

type permissionsRequest struct {
	AccountID int `json:"account_id"`
}

type permissionsResponse struct {
	Permissions []string `json:"permissions"`
}

// handleAccountPermissions lists the permissions granted to an account.
func (s *Server) handleAccountPermissions(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountPermissions)
	if !ok {
		return
	}
	var req permissionsRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	acc, err := s.store.FetchAccountByID(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	perms, err := s.store.ListAccountPermissions(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "permission lookup failed")
		return
	}
	writeJSON(w, http.StatusOK, permissionsResponse{Permissions: perms})
}

// --- session endpoints ---

type sessionCreateRequest struct {
	AccountID int `json:"account_id"`
}

type sessionTokenResponse struct {
	SessionID string `json:"session_id"`
	ExpiresAt string `json:"expires_at"`
}

// handleSessionCreate issues a session for an authenticated account.
func (s *Server) handleSessionCreate(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permSessionCreate)
	if !ok {
		return
	}
	var req sessionCreateRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	acc, err := s.store.FetchAccountByID(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil || (acc.BanUntil != nil && acc.BanUntil.After(time.Now())) {
		writeError(w, http.StatusUnauthorized, "account not loginable")
		return
	}
	raw, sess, err := s.store.CreateSession(context.Background(), req.AccountID, s.cfg.SessionTTL)
	if err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusUnauthorized, "account not loginable")
			return
		}
		dbError(w, err, "session create failed")
		return
	}
	writeJSON(w, http.StatusOK, sessionTokenResponse{SessionID: raw, ExpiresAt: sess.ExpiresAt.Format(time.RFC3339)})
}

type sessionValidateRequest struct {
	SessionID string `json:"session_id"`
}

// sessionValidateResponse is what the caller needs: is the session
// alive and which account does it belong to.
type sessionValidateResponse struct {
	Valid     bool   `json:"valid"`
	AccountID int    `json:"account_id,omitempty"`
	ExpiresAt string `json:"expires_at,omitempty"`
}

// handleSessionValidate checks a session token (e.g. a realm server
// checking a player's login).
func (s *Server) handleSessionValidate(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permSessionValidate)
	if !ok {
		return
	}
	var req sessionValidateRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	sess, err := s.store.ValidateSession(context.Background(), req.SessionID)
	if err != nil {
		dbError(w, err, "session lookup failed")
		return
	}
	if sess == nil {
		writeJSON(w, http.StatusOK, sessionValidateResponse{Valid: false})
		return
	}
	writeJSON(w, http.StatusOK, sessionValidateResponse{
		Valid:     true,
		AccountID: sess.AccountID,
		ExpiresAt: sess.ExpiresAt.Format(time.RFC3339),
	})
}

type sessionRevokeRequest struct {
	SessionID string `json:"session_id"`
}

// handleSessionRevoke marks a session as explicitly revoked. The row is kept
// (migration 014) so the revocation stays distinguishable from a normal expiry.
func (s *Server) handleSessionRevoke(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permSessionRevoke)
	if !ok {
		return
	}
	var req sessionRevokeRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if !isHexToken(req.SessionID) {
		writeError(w, http.StatusBadRequest, "session_id must be a 64-char hex token")
		return
	}
	if err := s.store.RevokeSession(context.Background(), req.SessionID); err != nil {
		dbError(w, err, "session revoke failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"revoked": true})
}

// --- session status batch (realm revocation poller, AUTH-02b) ---

// MaxSessionStatusBatch bounds one batch request. The realm poller splits
// larger sets; anything above this is rejected fail-closed instead of being
// silently truncated, so a realm can never act on a partial answer.
const MaxSessionStatusBatch = 250

// maxSessionStatusBatchBytes bounds THIS endpoint's request body, derived from
// MaxSessionStatusBatch and the accepted token format:
//
//	session_id   64 hex chars (isHexToken)          ->  64 bytes
//	per-entry JSON envelope, measured 106 B (int32 account_id)
//	                                           ->  128 bytes reserved
//	250 * 128                                    = 32000
//	+ slack for the "sessions" wrapper           =  1024
//	                                            = 33024  (32.25 KiB)
//
// 32 KiB = 32768 is therefore SLIGHTLY BELOW that arithmetic upper bound, a
// deliberate rounding down. It is nevertheless safe because the measured
// canonical payload is well under it: 26514 bytes at int32 account_id and
// 28764 bytes (28.09 KiB) at the int64 maximum — about 12% headroom. That is
// proven by TestSessionStatusBatchRejectsOversizeBody, which shows a full
// 250-entry batch is still accepted with all 250 results.
//
// The limit stays BELOW the server-wide maxBodyBytes (64 KiB, enforced in
// authorize() before any decoder); that server-wide limit is the outer backstop
// and is unchanged.
//
// Note: authorize() uses io.LimitReader, which TRUNCATES instead of
// rejecting. A chunked request (ContentLength == -1) therefore cannot be
// pre-checked and would surface as "invalid JSON body" (400) rather than 413.
// No partial processing happens either way, because the decoder sees the whole
// request as one value.
const maxSessionStatusBatchBytes int64 = 32 * 1024

type sessionStatusBatchRequest struct {
	Sessions []sessionStatusBatchItem `json:"sessions"`
}

type sessionStatusBatchItem struct {
	// SessionID is the opaque login token. It is used exclusively for hashing
	// and the lookup, never logged, and never echoed back.
	SessionID string `json:"session_id"`
	// AccountID is the caller's expectation. It is verified so a result can
	// never be attributed to the wrong account's connection.
	AccountID int `json:"account_id"`
}

type sessionStatusBatchResponse struct {
	Results []sessionStatusBatchResult `json:"results"`
}

type sessionStatusBatchResult struct {
	// Index is the position in the request, so correlation needs no secret.
	Index int `json:"index"`
	// Status is one of valid | expired | revoked | missing.
	Status SessionStatus `json:"status"`
	// AccountID is the authoritative owner (0 for missing). A realm must
	// treat a value different from its own expectation as "not my session".
	AccountID int `json:"account_id"`
}

// handleSessionStatusBatch resolves many sessions in one DB roundtrip and
// exposes the four-way status a realm needs to tell an explicit revocation
// apart from a normal TTL expiry (docs/Security.md AUTH-02a / AUTH-02b).
//
// Reuses permSessionValidate — no new permission. Tokens never appear in the
// response, and nothing about them is logged.
func (s *Server) handleSessionStatusBatch(w http.ResponseWriter, r *http.Request) {
	// Größenprüfung VOR authorize(): bei deklarierter Überschreitung wird der
	// Body gar nicht erst gelesen, es wird also nichts gebunden und nichts
	// teilverarbeitet. Der Server-weite maxBodyBytes bleibt die äußere Schranke.
	if r.ContentLength > maxSessionStatusBatchBytes {
		writeError(w, http.StatusRequestEntityTooLarge,
			"request body exceeds the session status batch limit")
		return
	}
	body, _, ok := s.authorize(w, r, permSessionValidate)
	if !ok {
		return
	}
	var req sessionStatusBatchRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if len(req.Sessions) == 0 {
		writeError(w, http.StatusBadRequest, "sessions must not be empty")
		return
	}
	if len(req.Sessions) > MaxSessionStatusBatch {
		// Fail closed: a truncated answer must never be interpreted as
		// "all still valid".
		writeError(w, http.StatusRequestEntityTooLarge,
			"too many sessions, max "+strconv.Itoa(MaxSessionStatusBatch))
		return
	}
	tokens := make([]string, len(req.Sessions))
	for i, it := range req.Sessions {
		if !isHexToken(it.SessionID) {
			writeError(w, http.StatusBadRequest, "session_id must be a 64-char hex token")
			return
		}
		tokens[i] = it.SessionID
	}
	statuses, accountIDs, err := s.store.BatchSessionStatus(context.Background(), tokens)
	if err != nil {
		dbError(w, err, "session status batch failed")
		return
	}
	out := sessionStatusBatchResponse{Results: make([]sessionStatusBatchResult, len(statuses))}
	for i := range statuses {
		out.Results[i] = sessionStatusBatchResult{
			Index:     i,
			Status:    statuses[i],
			AccountID: accountIDs[i],
		}
	}
	writeJSON(w, http.StatusOK, out)
}

// --- realms ---

type realmsResponse struct {
	Realms []Realm `json:"realms"`
}

// handleRealms lists the registered realms (no sensitive data).
func (s *Server) handleRealms(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet && r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "use GET or POST")
		return
	}
	_, _, ok := s.authorize(w, r, permRealmList)
	if !ok {
		return
	}
	realms, err := s.store.ListRealms(context.Background())
	if err != nil {
		dbError(w, err, "realm lookup failed")
		return
	}
	writeJSON(w, http.StatusOK, realmsResponse{Realms: realms})
}

// --- handoff ---

type handoffCreateRequest struct {
	AccountID int `json:"account_id"`
	RealmID   int `json:"realm_id"`
}

type handoffTokenResponse struct {
	HandoffToken string `json:"handoff_token"`
	ExpiresAt    string `json:"expires_at"`
}

// handleHandoffCreate issues a single-use handoff token that binds an
// account to a realm (the realm server consumes it, one use only).
func (s *Server) handleHandoffCreate(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permHandoffCreate)
	if !ok {
		return
	}
	var req handoffCreateRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	acc, err := s.store.FetchAccountByID(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	raw, exp, err := s.store.CreateHandoff(context.Background(), req.AccountID, req.RealmID, s.cfg.HandoffTTL)
	if err != nil {
		dbError(w, err, "handoff create failed")
		return
	}
	writeJSON(w, http.StatusOK, handoffTokenResponse{HandoffToken: raw, ExpiresAt: exp.Format(time.RFC3339)})
}

type handoffValidateRequest struct {
	HandoffToken string `json:"handoff_token"`
}

type handoffValidateResponse struct {
	Valid     bool `json:"valid"`
	AccountID int  `json:"account_id,omitempty"`
	RealmID   int  `json:"realm_id,omitempty"`
}

// handleHandoffValidate consumes a handoff token (validates AND marks
// it used in one step; a second call fails).
func (s *Server) handleHandoffValidate(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permHandoffValidate)
	if !ok {
		return
	}
	var req handoffValidateRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	h, err := s.store.ValidateHandoff(context.Background(), req.HandoffToken)
	if err != nil {
		dbError(w, err, "handoff lookup failed")
		return
	}
	if h == nil {
		writeJSON(w, http.StatusOK, handoffValidateResponse{Valid: false})
		return
	}
	if err := s.store.UseHandoff(context.Background(), req.HandoffToken); err != nil {
		if err == sql.ErrNoRows {
			writeJSON(w, http.StatusOK, handoffValidateResponse{Valid: false})
			return
		}
		dbError(w, err, "handoff consume failed")
		return
	}
	writeJSON(w, http.StatusOK, handoffValidateResponse{Valid: true, AccountID: h.AccountID, RealmID: h.RealmID})
}

// --- world server ---

type worldAuthRequest struct {
	ServerID   int    `json:"server_id"`
	Credential string `json:"credential"`
}

type worldAuthResponse struct {
	Valid   bool   `json:"valid"`
	RealmID int    `json:"realm_id,omitempty"`
	Name    string `json:"name,omitempty"`
}

// handleWorldAuthenticate checks the credential of a registered
// world server (constant-time compare against the stored value).
//
// LEGACY: kein separater Worldserver mehr (Kette Auth/API → Login → Realm).
// Der Realm-Server authentifiziert Spieler per Handoff; diese Endpunkte
// werden von keinem Dienst verwendet und bleiben nur kompatibel bestehen.
func (s *Server) handleWorldAuthenticate(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permWorldAuthenticate)
	if !ok {
		return
	}
	var req worldAuthRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if req.Credential == "" {
		writeError(w, http.StatusBadRequest, "credential required")
		return
	}
	ws, err := s.store.AuthenticateWorld(context.Background(), req.ServerID, req.Credential)
	if err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusUnauthorized, "world server not authenticated")
			return
		}
		dbError(w, err, "world auth lookup failed")
		return
	}
	writeJSON(w, http.StatusOK, worldAuthResponse{Valid: true, RealmID: ws.RealmID, Name: ws.Name})
}

type worldHeartbeatRequest struct {
	ServerID       int    `json:"server_id"`
	Credential     string `json:"credential"`
	Version        string `json:"version"`
	CurrentPlayers int    `json:"current_players"`
	Ok             bool   `json:"ok"`
}

// handleWorldHeartbeat re-authenticates the world server and records
// its status. ok=false marks the server offline.
//
// LEGACY: siehe handleWorldAuthenticate.
func (s *Server) handleWorldHeartbeat(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permWorldHeartbeat)
	if !ok {
		return
	}
	var req worldHeartbeatRequest
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if req.Credential == "" {
		writeError(w, http.StatusBadRequest, "credential required")
		return
	}
	if _, err := s.store.AuthenticateWorld(context.Background(), req.ServerID, req.Credential); err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusUnauthorized, "world server not authenticated")
			return
		}
		dbError(w, err, "world auth lookup failed")
		return
	}
	if err := s.store.RecordHeartbeat(context.Background(), req.ServerID, req.Version, req.CurrentPlayers, req.Ok); err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusNotFound, "world server not found")
			return
		}
		dbError(w, err, "heartbeat record failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"recorded": true})
}
