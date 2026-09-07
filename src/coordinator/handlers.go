package main

import (
	"net/http"
	"strconv"
	"strings"
	"time"
)

// jobSubmission is the public realm→coordinator payload. The realm id
// is taken from the authenticated service credential, never from the
// payload (fail-closed against spoofing).
type jobSubmission struct {
	JobType     string     `json:"job_type"`
	JobID       string     `json:"job_id"`
	RealmID     string     `json:"realm_id"`
	CharacterID string     `json:"character_id,omitempty"`
	PlayerID    string     `json:"player_id,omitempty"`
	NPCID       string     `json:"npc_id,omitempty"`
	Text        string     `json:"text"`
	Context     jobContext `json:"context,omitempty"`
}

// handleHealth and handleStatus are open (no signature, no account
// data) — same convention as the other Andora Go services.
func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func (s *Server) handleStatus(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, s.queue.metrics())
}

// handleSubmitJob accepts one KI job (§3/§4). Flow: signature check →
// input validation (sections 7/9) → queue-capacity check (§4) →
// per-player cooldown (§6) → persistent enqueue (§14–16) →
// asynchronous processing.
func (s *Server) handleSubmitJob(w http.ResponseWriter, r *http.Request) {
	body, cred, ok := s.authorize(w, r, permJobsSubmit)
	if !ok {
		return
	}
	var in jobSubmission
	if !decodeJSON(body, &in) {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if in.RealmID != "" && in.RealmID != cred.ID {
		writeError(w, http.StatusBadRequest, CodeInputInvalid)
		return
	}
	j, err := NewJobFromPayload(in, s.cfg, time.Now())
	if err != nil {
		writeError(w, http.StatusBadRequest, CodeUnknownJobType)
		return
	}
	j.RealmID = cred.ID

	v := newValidator(s.cfg)
	if rv := v.inputValidation(j); !rv.OK {
		writeError(w, http.StatusBadRequest, rv.Reason)
		return
	}

	if pending, _, _, err := s.store.Stats(); err == nil && pending >= s.cfg.QueueMaxSize {
		writeError(w, http.StatusServiceUnavailable, CodeQueueFull)
		return
	}

	if err := s.queue.accept(j, time.Now()); err != nil {
		switch e := err.(type) {
		case *cooldownError:
			w.Header().Set("Retry-After", strconv.Itoa(max(1, int(e.retry.Seconds()))))
			writeError(w, http.StatusTooManyRequests, CodeCooldown)
		default:
			if err == ErrDuplicate {
				writeError(w, http.StatusConflict, CodeJobExists)
				return
			}
			writeError(w, http.StatusInternalServerError, "queue write failed")
		}
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{
		"code":      CodeQueued,
		"realm_id":  j.RealmID,
		"job_id":    j.JobID,
		"job_type":  j.JobType,
		"queued_at": j.CreatedAt,
	})
}

// handleQueryJob answers §25/§26: does a job still exist inside the
// coordinator's own scope, and what is its current status? The realm
// interprets the answer with its own data.
func (s *Server) handleQueryJob(w http.ResponseWriter, r *http.Request) {
	_, cred, ok := s.authorize(w, r, permJobsQuery)
	if !ok {
		return
	}
	id := strings.TrimPrefix(r.URL.Path, "/v1/jobs/")
	if !validJobID(id) {
		writeError(w, http.StatusBadRequest, "invalid job id")
		return
	}
	j, exists, err := s.store.GetByKey(cred.ID, id)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "job lookup failed")
		return
	}
	if !exists {
		writeJSON(w, http.StatusNotFound, map[string]any{
			"exists":   false,
			"realm_id": cred.ID,
			"job_id":   id,
		})
		return
	}
	out := map[string]any{
		"exists":     true,
		"realm_id":   cred.ID,
		"job_id":     j.JobID,
		"job_type":   j.JobType,
		"status":     j.Status,
		"created_at": j.CreatedAt,
	}
	if j.Status == jobCompleted {
		out["result"] = j.Result
	}
	if j.Status == jobFailed {
		out["error_code"] = j.ErrorCode
		out["error_msg"] = codeText(j.ErrorCode)
	}
	writeJSON(w, http.StatusOK, out)
}
