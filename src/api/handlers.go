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
//
// Two-factor extensions:
//   - device_token:   presented to be recognized as a confirmed device
//     (password-only login is granted for confirmed devices)
//   - totp_code:      the 6-digit TOTP for unconfirmed devices
//   - recovery_code:  single-use emergency code (never confirms a device)
//   - device_label / trust_device: confirm the presenting device (max 3)
//
// Failure shapes stay distinct-by-design:
//   - {valid:false}                          -> missing account / ban / wrong password / bad or replayed recovery-code
//   - {valid:false, two_factor_required}     -> TOTP missing, wrong, expired or replayed
//   - {valid:false, parental_blocked}        -> today's supervised play budget is exhausted
type verifyResponse struct {
	Valid             bool   `json:"valid"`
	AccountID         int    `json:"account_id,omitempty"`
	SessionID         string `json:"session_id,omitempty"`
	ExpiresAt         string `json:"expires_at,omitempty"`
	DeviceToken       string `json:"device_token,omitempty"`
	TwoFactorRequired bool   `json:"two_factor_required,omitempty"`
	ParentalBlocked   bool   `json:"parental_blocked,omitempty"`
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
		DeviceToken     string `json:"device_token"`
		TOTPCode        string `json:"totp_code"`
		RecoveryCode    string `json:"recovery_code"`
		DeviceLabel     string `json:"device_label"`
		TrustDevice     bool   `json:"trust_device"`
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

	// --- device recognition ---
	//
	// A confirmed device unlocks password-only login; an unknown device
	// triggers the second factor below.
	deviceConfirmed := false
	if req.DeviceToken != "" {
		has, err := s.store.HasTrustedDevice(ctx, acc.ID, req.DeviceToken)
		if err != nil {
			writeError(w, http.StatusInternalServerError, "device lookup failed")
			return
		}
		deviceConfirmed = has
	}

	// --- second factor (only when 2FA is on and the device is unknown) ---
	//
	// The second factor is a single-use recovery code OR the 6-digit
	// TOTP. A recovery code deliberately does NOT confirm the device.
	if acc.TwoFactorEnabled && !deviceConfirmed {
		need := func() {
			writeJSON(w, http.StatusOK,
				verifyResponse{Valid: false, TwoFactorRequired: true})
		}
		if req.RecoveryCode != "" {
			// Single-use recovery code: invalid or already used -> same
			// indistinguishable fail shape as a bad password.
			if err := s.store.UseRecoveryCode(ctx, acc.ID, req.RecoveryCode); err != nil {
				fail()
				return
			}
		} else {
			var rawSecret []byte
			if acc.TwoFactorSecret != nil {
				plain, err := aesGCMDecrypt(s.cfg.EncryptionKey, acc.TwoFactorSecret)
				if err != nil {
					need()
					return
				}
				rawSecret = plain
			}
			if len(rawSecret) == 0 {
				need()
				return
			}
			counter, ok := evalTOTPCode(rawSecret, req.TOTPCode, now)
			if !ok {
				need()
				return
			}
			if acc.LastTOTPCounter != nil && counter <= int64(*acc.LastTOTPCounter) {
				// replayed / stale code
				need()
				return
			}
			if err := s.store.SetLastTOTP(ctx, acc.ID, int(counter), now); err != nil {
				writeError(w, http.StatusInternalServerError, "totp state update failed")
				return
			}
		}
	}

	// --- (optional) device confirmation on this login ---
	var confirmedDevice string
	if req.TrustDevice && !deviceConfirmed && req.DeviceToken != "" {
		label := trimSpace(req.DeviceLabel)
		if label == "" {
			label = "device"
		}
		if len(label) > 64 {
			label = label[:64]
		}
		if _, err := s.store.AddTrustedDevice(ctx, acc.ID, req.DeviceToken, label); err != nil {
			if err == ErrMaxDevices {
				writeJSON(w, http.StatusConflict,
					map[string]string{"error": "max_devices_reached"})
				return
			}
			writeError(w, http.StatusInternalServerError, "device confirm failed")
			return
		}
		deviceConfirmed = true
		confirmedDevice = req.DeviceToken
	}

	// --- parental control: no new login while today's supervised
	// budget is exhausted (pure read, no session yet: a fresh login
	// never starts the grace buffer).
	pst, err := s.computeParentalState(ctx, acc, false, false, time.Now())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "parental lookup failed")
		return
	}
	if pst.Enabled && pst.Blocked {
		writeJSON(w, http.StatusOK, verifyResponse{Valid: false, ParentalBlocked: true})
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
	// Refresh the 30-day inactivity window of the presenting confirmed
	// device. Best effort: a failed touch never fails the login itself.
	if deviceConfirmed && req.DeviceToken != "" {
		_ = s.store.TouchTrustedDevice(ctx, acc.ID, req.DeviceToken)
	}
	writeJSON(w, http.StatusOK, verifyResponse{
		Valid:       true,
		AccountID:   acc.ID,
		SessionID:   rawID,
		ExpiresAt:   sess.ExpiresAt.Format(time.RFC3339),
		DeviceToken: confirmedDevice,
	})
}
