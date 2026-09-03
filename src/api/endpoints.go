package main

import (
	"context"
	"crypto/rand"
	"database/sql"
	"encoding/json"
	"net/http"
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

// --- /status ---

type statusResponse struct {
	Status string `json:"status"`
	Uptime string `json:"uptime"`
}

// handleStatus is public (no service signature): liveness + uptime.
func (s *Server) handleStatus(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, statusResponse{
		Status: "ok",
		Uptime: time.Since(s.start).Round(time.Second).String(),
	})
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

// handleSessionRevoke deletes a session token.
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
