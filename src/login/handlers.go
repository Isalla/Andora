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
	type clientRealm struct {
		Realm
		HTTPURL string `json:"http_url"`
	}
	out := make([]clientRealm, 0, len(realms))
	for _, realm := range realms {
		out = append(out, clientRealm{Realm: realm, HTTPURL: s.cfg.RealmHTTP[realm.ID]})
	}
	writeJSON(w, http.StatusOK, map[string]any{"realms": out})
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

// --- POST /parental ---
//
// Client-facing proxy for the parental management surface
// (docs/parental_control.md): the presenting session is validated, the
// account_id is taken from that validation (never from the client), the
// action is whitelisted and forwarded to the Auth-API with the service's
// own credentials. The client never sees or carries those credentials.
// PINs pass through in the payload and are never logged; the Auth-API
// answer is passed through unchanged (parental answers never contain
// PINs, tokens or e-mail — notifications/deliver is not proxied).
func (s *Server) handleParental(w http.ResponseWriter, r *http.Request) {
	body, ok := readBody(w, r)
	if !ok {
		return
	}
	var in map[string]any
	if !decodeJSON(body, &in) {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	action := asString(in, "action")
	authPath := parentalAuthPath(action)
	if authPath == "" {
		writeError(w, http.StatusBadRequest, "unknown action")
		return
	}
	sessionID := asString(in, "session_id")
	if sessionID == "" {
		writeError(w, http.StatusBadRequest, "session_id required")
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
	var fwd map[string]any = map[string]any{"account_id": sess.AccountID}
	for _, k := range parentalFields(action) {
		if v, ok := in[k]; ok {
			fwd[k] = v
		}
	}
	status, raw, err := s.auth.postRaw(authPath, fwd)
	if err != nil {
		authUnavailable(w, err)
		return
	}
	if status != http.StatusOK && status != http.StatusCreated {
		writeError(w, status, parentalErrorFor(status))
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_, _ = w.Write(raw)
}

func asString(in map[string]any, key string) string {
	if v, ok := in[key]; ok {
		if s, ok := v.(string); ok {
			return s
		}
	}
	return ""
}

// parentalAuthPath whitelists the client-facing parental actions and
// maps them to the Auth-API paths. Anything else (including
// notifications/deliver) is rejected before any service call.
func parentalAuthPath(action string) string {
	switch action {
	case "status":
		return "/parental/status"
	case "setup":
		return "/parental/setup"
	case "update":
		return "/parental/update"
	case "remove":
		return "/parental/remove"
	case "pin_change":
		return "/parental/pin/change"
	case "pin_verify":
		return "/parental/pin/verify"
	case "periods":
		return "/parental/periods"
	case "periods_add":
		return "/parental/periods/add"
	case "periods_remove":
		return "/parental/periods/remove"
	case "exceptions_add":
		return "/parental/exceptions/add"
	case "exceptions_remove":
		return "/parental/exceptions/remove"
	}
	return ""
}

// parentalFields lists the payload fields forwarded per action; the
// client's session_id and any client-supplied account_id are never
// copied (account_id comes from the validated session only).
func parentalFields(action string) []string {
	switch action {
	case "status", "periods":
		return []string{}
	case "setup", "update":
		return []string{
			"parent_pin", "parent_email", "chat_enabled", "voice_enabled",
			"warning_minutes", "monday_minutes", "tuesday_minutes",
			"wednesday_minutes", "thursday_minutes", "friday_minutes",
			"saturday_minutes", "sunday_minutes",
		}
	case "remove", "pin_verify":
		return []string{"parent_pin"}
	case "pin_change":
		return []string{"old_pin", "new_pin"}
	case "periods_add":
		return []string{
			"parent_pin", "starts_at", "ends_at", "monday_minutes",
			"tuesday_minutes", "wednesday_minutes", "thursday_minutes",
			"friday_minutes", "saturday_minutes", "sunday_minutes",
		}
	case "periods_remove":
		return []string{"parent_pin", "id"}
	case "exceptions_add":
		return []string{"parent_pin", "date", "extra_minutes", "override_minutes"}
	case "exceptions_remove":
		return []string{"parent_pin", "date"}
	}
	return []string{}
}

// parentalErrorFor maps a non-2xx Auth-API status to a static client
// message; the Auth-API's own error text is never relayed (it is static
// today, but the mapping keeps secrets out of the answer by design).
func parentalErrorFor(status int) string {
	switch status {
	case http.StatusBadRequest:
		return "invalid parental payload"
	case http.StatusUnauthorized:
		return "parent pin does not match"
	case http.StatusNotFound:
		return "parental control not found"
	case http.StatusConflict:
		return "parental conflict"
	}
	return "auth service unavailable"
}
