package main

import (
	"context"
	"database/sql"
	"encoding/json"
	"log"
	"net/http"
	"strings"
	"time"
)

// Two-factor + trusted-device surface. Handlers for the new endpoints
// (/auth/verify is extended in handlers.go):
//
//	/twofactor/status   -> {enabled, recovery_codes_available}
//	/twofactor/setup    -> {enabled, provisioning_uri, recovery_codes}
//	/twofactor/enable   -> {enabled}
//	/twofactor/disable  -> {enabled}
//	/twofactor/reset    -> {enabled, provisioning_uri, recovery_codes}
//	/devices/list       -> {devices:[{id,label,confirmed_at}]}
//	/devices/revoke     -> {revoked}
//	/security/events    -> {events:[{id,event_type,created_at}]}
//
// Bodies stay minimal and deterministic; no secrets, no raw Base32,
// no token_hash or e-mail data are ever returned.

// --- shared helpers ---

func trimSpace(s string) string { return strings.TrimSpace(s) }

// recordEvent logs a best-effort security event; it is never a hard
// failure. After P-34 it has NO 2FA caller left: the security-relevant 2FA
// operations write their event inside their own transaction, so a failing
// event write rolls the whole operation back. The remaining callers are the
// Parental paths, whose best-effort semantics stay separate and are
// documented as P-35.
//
// P-35: the write still never fails the caller, but a lost history row is no
// longer silent. Exactly one structured WARN is emitted per failed write and
// the parental state change stays committed — no rollback, no error answer,
// no retry. The stage is fixed to event_write, which denotes the write to
// security_events.
func (s *Server) recordEvent(ctx context.Context, eventType string, accountID int) {
	if err := s.store.RecordSecurityEvent(ctx, eventType, &accountID); err != nil {
		log.Print(parentalHistoryFailureLine(eventType, accountID, "event_write"))
	}
}

// --- /twofactor/status ---

type twofactorStatusResponse struct {
	Enabled                bool `json:"enabled"`
	RecoveryCodesAvailable bool `json:"recovery_codes_available"`
}

// handleTwofactorStatus reports the 2FA state without exposing any
// secret.
func (s *Server) handleTwofactorStatus(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountTwoFactor)
	if !ok {
		return
	}
	var req struct {
		AccountID int `json:"account_id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	acc, err := s.store.FetchAccountByID(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	avail := false
	if acc.TwoFactorEnabled {
		n, err := s.store.CountRecoveryCodes(ctx, req.AccountID)
		if err != nil {
			dbError(w, err, "count recovery codes failed")
			return
		}
		avail = n > 0
	}
	writeJSON(w, http.StatusOK, twofactorStatusResponse{Enabled: acc.TwoFactorEnabled, RecoveryCodesAvailable: avail})
}

// --- /twofactor/setup ---

type twofactorSetupResponse struct {
	Enabled         bool     `json:"enabled"`
	ProvisioningURI string   `json:"provisioning_uri"`
	RecoveryCodes   []string `json:"recovery_codes"`
}

// handleTwofactorSetup enables 2FA, stores the encrypted TOTP secret,
// issues the provisioning URI and generates the 10 single-use recovery
// codes shown exactly once.
func (s *Server) handleTwofactorSetup(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountTwoFactor)
	if !ok {
		return
	}
	var req struct {
		AccountID int `json:"account_id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	acc, err := s.store.FetchAccountByID(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	if acc.TwoFactorEnabled {
		writeError(w, http.StatusConflict, "two-factor already enabled")
		return
	}
	secretRaw, _, err := newTOTPSecret()
	if err != nil {
		writeError(w, http.StatusInternalServerError, "totp secret generation failed")
		return
	}
	enc, err := aesGCMEncrypt(s.cfg.EncryptionKey, secretRaw)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "totp secret encryption failed")
		return
	}
	codes := newRecoveryCodes()
	// One transaction: secret, codes, enabled flag and the security event
	// commit together. The store gate also decides the conflict, so the
	// pre-read above is only a fast path (P-34).
	if err := s.store.SetupTwoFactor(ctx, acc.ID, enc, codes); err != nil {
		if err == ErrTwoFactorAlreadySetUp {
			writeError(w, http.StatusConflict, "two-factor already enabled")
			return
		}
		dbError(w, err, "setup two-factor failed")
		return
	}
	writeJSON(w, http.StatusOK, twofactorSetupResponse{
		Enabled:         true,
		ProvisioningURI: provisioningURI(secretRaw, acc.ID),
		RecoveryCodes:   codes,
	})
}

// --- /twofactor/enable ---

// handleTwofactorEnable turns 2FA on. Same effect as setup but keeps
// the existing secret/codes (used after a disable).
func (s *Server) handleTwofactorEnable(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountTwoFactor)
	if !ok {
		return
	}
	var req struct {
		AccountID int `json:"account_id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	acc, err := s.store.FetchAccountByID(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	if _, err := s.store.EnableTwoFactor(ctx, acc.ID); err != nil {
		dbError(w, err, "enable two-factor failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"enabled": true})
}

// --- /twofactor/disable ---

// handleTwofactorDisable turns 2FA off and clears the secret, the
// recovery codes and all trusted devices in ONE transaction together with
// the security event (sessions remain, per the revocation policy).
func (s *Server) handleTwofactorDisable(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountTwoFactor)
	if !ok {
		return
	}
	var req struct {
		AccountID int `json:"account_id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	acc, err := s.store.FetchAccountByID(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	if _, err := s.store.DisableTwoFactor(ctx, acc.ID); err != nil {
		dbError(w, err, "disable two-factor failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"enabled": false})
}

// --- /twofactor/reset ---

// handleTwofactorReset rotates the state completely: new secret, new
// codes, revokes sessions + trusted devices, re-arms 2FA — all in ONE
// transaction together with the security event (P-34).
//
// The encrypted secret read above is handed to the store as the expected
// starting state (compare-and-swap). A second request that read the SAME
// stored secret therefore loses with 409 and receives neither a
// provisioning URI nor recovery codes. A reset that starts after the
// previous commit read the already rotated secret and is a new valid
// operation: last commit wins.
func (s *Server) handleTwofactorReset(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permAccountTwoFactor)
	if !ok {
		return
	}
	var req struct {
		AccountID int `json:"account_id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	acc, err := s.store.FetchAccountByID(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "account lookup failed")
		return
	}
	if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	secretRaw, _, err := newTOTPSecret()
	if err != nil {
		writeError(w, http.StatusInternalServerError, "totp secret generation failed")
		return
	}
	enc, err := aesGCMEncrypt(s.cfg.EncryptionKey, secretRaw)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "totp secret encryption failed")
		return
	}
	codes := newRecoveryCodes()
	changed, err := s.store.ResetTwoFactor(ctx, acc.ID, acc.TwoFactorSecret, enc, codes)
	if err != nil {
		dbError(w, err, "reset two-factor failed")
		return
	}
	if !changed {
		writeError(w, http.StatusConflict, "two-factor state changed; retry")
		return
	}
	writeJSON(w, http.StatusOK, twofactorSetupResponse{
		Enabled:         true,
		ProvisioningURI: provisioningURI(secretRaw, acc.ID),
		RecoveryCodes:   codes,
	})
}

// --- /devices/list ---

type deviceListEntry struct {
	ID          int    `json:"id"`
	Label       string `json:"label"`
	ConfirmedAt string `json:"confirmed_at"`
}

type deviceListResponse struct {
	Devices []deviceListEntry `json:"devices"`
}

// handleDevicesList lists the confirmed devices.
func (s *Server) handleDevicesList(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permDeviceList)
	if !ok {
		return
	}
	var req struct {
		AccountID int `json:"account_id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	devs, err := s.store.ListTrustedDevices(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "list trusted devices failed")
		return
	}
	out := deviceListResponse{}
	for _, d := range devs {
		out.Devices = append(out.Devices, deviceListEntry{
			ID:          d.ID,
			Label:       d.Label,
			ConfirmedAt: d.ConfirmedAt.Format(time.RFC3339),
		})
	}
	writeJSON(w, http.StatusOK, out)
}

// --- /devices/revoke ---

// handleDevicesRevoke revokes one confirmed device.
func (s *Server) handleDevicesRevoke(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permDeviceRevoke)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		Token     string `json:"device_token"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if req.Token == "" {
		writeError(w, http.StatusBadRequest, "device_token required")
		return
	}
	if err := s.store.RevokeTrustedDevice(context.Background(), req.AccountID, req.Token); err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusNotFound, "device not found")
			return
		}
		dbError(w, err, "revoke device failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"revoked": true})
}

// --- /security/events ---

type securityEventEntry struct {
	ID        int    `json:"id"`
	EventType string `json:"event_type"`
	CreatedAt string `json:"created_at"`
}

type securityEventsResponse struct {
	Events []securityEventEntry `json:"events"`
}

// handleSecurityEvents lists the account's security events.
func (s *Server) handleSecurityEvents(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permSecurityEvents)
	if !ok {
		return
	}
	var req struct {
		AccountID int `json:"account_id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	evs, err := s.store.ListSecurityEvents(context.Background(), req.AccountID)
	if err != nil {
		dbError(w, err, "list security events failed")
		return
	}
	out := securityEventsResponse{}
	for _, e := range evs {
		out.Events = append(out.Events, securityEventEntry{
			ID:        e.ID,
			EventType: e.EventType,
			CreatedAt: e.CreatedAt.Format(time.RFC3339),
		})
	}
	writeJSON(w, http.StatusOK, out)
}
