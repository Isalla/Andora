package main

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"time"
)

// verifyResponse is the minimal result of a verify. On success the API
// also creates the login session and reports it. No sensitive account
// data (password hash, email, lookup hash, ban, ...) is ever returned.
type verifyResponse struct {
	Valid     bool   `json:"valid"`
	AccountID int    `json:"account_id,omitempty"`
	SessionID string `json:"session_id,omitempty"`
	ExpiresAt string `json:"expires_at,omitempty"`
}

// handleHealth reports liveness without exposing configuration or
// account data.
func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

// handleAuthVerify is the login check: one request, one store lookup,
// one password check, one session write. Responses on any failure
// are indistinguishable: the account may be missing, banned or simply
// have typed the wrong password.
func (s *Server) handleAuthVerify(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountAuthenticate)
	if !ok {
		return
	}
	var req struct {
		Username        string `json:"username"`
		Password        string `json:"password"`
		EmailLookupHash string `json:"email_lookup_hash"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if req.Password == "" {
		writeError(w, http.StatusBadRequest, "password required")
		return
	}
	var column, value interface{}
	switch {
	case req.Username != "":
		column = "username"
		value = req.Username
	case req.EmailLookupHash != "":
		raw, err := hex.DecodeString(req.EmailLookupHash)
		if err != nil || len(raw) != 32 {
			writeError(w, http.StatusBadRequest, "email_lookup_hash must be 32 bytes hex")
			return
		}
		column = "email_lookup_hash"
		value = raw
	default:
		writeError(w, http.StatusBadRequest, "username or email_lookup_hash required")
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), verifyDBTimeout)
	defer cancel()
	acc, err := s.store.FetchAccount(ctx, column.(string), value)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "account lookup failed")
		return
	}
	now := time.Now()
	fail := func() {
		writeJSON(w, http.StatusOK, verifyResponse{Valid: false})
	}
	if acc == nil || (acc.BanUntil != nil && acc.BanUntil.After(now)) ||
		!VerifyPassword(acc.PasswordHash, req.Password) {
		fail()
		return
	}
	rawID, sess, err := s.store.CreateSession(ctx, acc.ID, s.cfg.SessionTTL)
	if err != nil {
		if err == sql.ErrNoRows {
			fail()
			return
		}
		writeError(w, http.StatusInternalServerError, "session create failed")
		return
	}
	if err := s.store.TouchLogin(ctx, acc.ID); err != nil {
		_ = s.store.RevokeSession(ctx, rawID)
		writeError(w, http.StatusInternalServerError, "last_login update failed")
		return
	}
	writeJSON(w, http.StatusOK, verifyResponse{
		Valid:     true,
		AccountID: acc.ID,
		SessionID: rawID,
		ExpiresAt: sess.ExpiresAt.Format(time.RFC3339),
	})
}
