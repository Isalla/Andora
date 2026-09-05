package main

import (
	"net/http"
	"time"
)

// --- /health, /status (public liveness) ---

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func (s *Server) handleStatus(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{
		"status": "ok",
		"uptime": time.Since(s.start).Round(time.Second).String(),
	})
}

// --- POST /login ---
//
// Forwards the credential check to the Auth/API-Service (/auth/verify)
// and returns its answer shape unchanged: {valid, account_id,
// session_id, expires_at, device_token, two_factor_required,
// parental_blocked}. The login service never sees password hashes,
// e-mails or encryption keys; it only forwards the presented
// credentials and relays the minimal result.
func (s *Server) handleLogin(w http.ResponseWriter, r *http.Request) {
	body, ok := readBody(w, r)
	if !ok {
		return
	}
	var in map[string]any
	if !decodeJSON(body, &in) {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	pw, _ := in["password"].(string)
	if pw == "" {
		writeError(w, http.StatusBadRequest, "password required")
		return
	}
	if _, hasUser := in["username"]; !hasUser {
		if _, hasMail := in["email_lookup_hash"]; !hasMail {
			writeError(w, http.StatusBadRequest, "username or email_lookup_hash required")
			return
		}
	}
	out, err := s.auth.Verify(in)
	if err != nil {
		authUnavailable(w, err)
		return
	}
	writeJSON(w, http.StatusOK, out)
}

// --- POST /logout ---
//
// Revokes the presenting login session via the Auth-API. Token
// possession authorizes the call (same as the Auth-API itself).
func (s *Server) handleLogout(w http.ResponseWriter, r *http.Request) {
	body, ok := readBody(w, r)
	if !ok {
		return
	}
	var in struct {
		SessionID string `json:"session_id"`
	}
	if !decodeJSON(body, &in) || in.SessionID == "" {
		writeError(w, http.StatusBadRequest, "session_id required")
		return
	}
	if err := s.auth.RevokeSession(in.SessionID); err != nil {
		authUnavailable(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"revoked": true})
}

// --- GET|POST /realms ---
//
// Validates the presenting session and returns the registered realms
// (business data only: id, name, language, region, enabled,
// fresh_start_until, transfer_policy). The account itself belongs to
// no realm; realm choice stays with the player.
func (s *Server) handleRealms(w http.ResponseWriter, r *http.Request) {
	sessionID := r.URL.Query().Get("session_id")
	if r.Method == http.MethodPost {
		body, ok := readBody(w, r)
		if !ok {
			return
		}
		var in struct {
			SessionID string `json:"session_id"`
		}
		if !decodeJSON(body, &in) {
			writeError(w, http.StatusBadRequest, "invalid JSON body")
			return
		}
		sessionID = in.SessionID
	} else if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "use GET or POST")
		return
	}
	if sessionID == "" {
		writeError(w, http.StatusUnauthorized, "session_id required")
		return
	}
	sess, err := s.auth.ValidateSession(sessionID)
	if err != nil {
		authUnavailable(w, err)
		return
	}
	if !sess.Valid {
		writeError(w, http.StatusUnauthorized, "invalid session")
		return
	}
	realms, err := s.auth.ListRealms()
	if err != nil {
		authUnavailable(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"realms": realms})
}

// --- POST /handoff ---
//
// Issues the secure handover to a realm: the presenting session is
// validated, the account_id is taken from that validation (never from
// the client), the realm must exist and be enabled, then a single-use
// handoff token is created via the Auth-API. The answer additionally
// carries the realm's game address (ws_url) from the operator's
// REALM_WS_URLS mapping (empty when unmapped).
func (s *Server) handleHandoff(w http.ResponseWriter, r *http.Request) {
	body, ok := readBody(w, r)
	if !ok {
		return
	}
	var in struct {
		SessionID string `json:"session_id"`
		RealmID   int    `json:"realm_id"`
	}
	if !decodeJSON(body, &in) || in.SessionID == "" {
		writeError(w, http.StatusBadRequest, "session_id required")
		return
	}
	if in.RealmID <= 0 {
		writeError(w, http.StatusBadRequest, "realm_id required")
		return
	}
	sess, err := s.auth.ValidateSession(in.SessionID)
	if err != nil {
		authUnavailable(w, err)
		return
	}
	if !sess.Valid {
		writeError(w, http.StatusUnauthorized, "invalid session")
		return
	}
	realms, err := s.auth.ListRealms()
	if err != nil {
		authUnavailable(w, err)
		return
	}
	var target *Realm
	for i := range realms {
		if realms[i].ID == in.RealmID {
			target = &realms[i]
			break
		}
	}
	if target == nil {
		writeError(w, http.StatusNotFound, "unknown realm")
		return
	}
	if !target.Enabled {
		writeError(w, http.StatusForbidden, "realm disabled")
		return
	}
	h, err := s.auth.CreateHandoff(sess.AccountID, target.ID)
	if err != nil {
		authUnavailable(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"handoff_token": h.HandoffToken,
		"expires_at":    h.ExpiresAt,
		"realm_id":      target.ID,
		"ws_url":        s.cfg.RealmWS[target.ID],
	})
}
