package main

import (
	"context"
	"database/sql"
	"encoding/json"
	"log"
	"net/http"
	"time"
)

// Parental-control endpoints. The full rule set is account-bound and
// enforced server-side; the realm server only consumes these endpoints
// with its own service credential (it never touches the auth DB).
//
//	/parental/status                -> effective status (realm/login poll)
//	/parental/setup                 -> create control + PIN (+optional e-mail)
//	/parental/update                -> change rules/e-mail (PIN required)
//	/parental/remove                -> delete control (PIN required)
//	/parental/pin/change            -> rotate the parent PIN
//	/parental/pin/verify            -> {valid} for the in-game parent panel
//	/parental/extension             -> once-per-day +1h (PIN required)
//	/parental/periods               -> list special periods
//	/parental/periods/add           -> add special period (PIN required)
//	/parental/periods/remove        -> remove special period (PIN required)
//	/parental/exceptions/add        -> set day exception (PIN required)
//	/parental/exceptions/remove     -> remove day exception (PIN required)
//	/parental/notifications         -> pending change receipts
//	/parental/notifications/deliver -> mark delivered, return address once
//
// No PIN, PIN hash, token or e-mail is ever returned except through the
// deliver endpoint (plaintext address, exactly once per receipt).

// --- shared validation ---

// validParentPin: 4-16 digits (checked as string, never logged).
func validParentPin(p string) bool {
	if len(p) < 4 || len(p) > 16 {
		return false
	}
	for _, c := range p {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}

// validDayMinutes: 0 = unlimited, otherwise 1..1440 minutes per day.
func validDayMinutes(m int) bool { return m >= 0 && m <= 1440 }

func validWeek(w [7]int) bool {
	for _, m := range w {
		if !validDayMinutes(m) {
			return false
		}
	}
	return true
}

func parseParentalDate(s string) (time.Time, bool) {
	t, err := time.Parse("2006-01-02", s)
	if err != nil {
		return time.Time{}, false
	}
	return t, true
}

// notifyParental enqueues one change receipt; it is never a hard
// failure (best effort, like security events). Without a recipient
// address there is nothing to deliver to.
//
// P-35: a lost receipt no longer stays silent — exactly one structured WARN
// per failed write, while the parental state change stays committed. No
// rollback, no error answer, no retry. The early return without a recipient
// remains a silent, error-free no-op. The stage is fixed to
// notification_write, which denotes the write to parental_notifications; it
// is what tells this history apart from security_events.
func (s *Server) notifyParental(ctx context.Context, accountID int, eventType, setting, oldVal, newVal string, recipientEnc []byte) {
	if len(recipientEnc) == 0 {
		return
	}
	oldVal = truncateNotify(oldVal)
	newVal = truncateNotify(newVal)
	if _, err := s.store.CreateParentalNotification(ctx, &ParentalNotification{
		AccountID: accountID, EventType: eventType, SettingName: setting,
		OldValue: oldVal, NewValue: newVal, RecipientEmailEnc: recipientEnc,
	}); err != nil {
		log.Print(parentalHistoryFailureLine(eventType, accountID, "notification_write"))
	}
}

func truncateNotify(v string) string {
	if len(v) > 255 {
		return v[:255]
	}
	return v
}

// fetchParentalAccount loads the account and its control row; 404 when
// either is missing (callers that need receipts after removal use the
// account-only path instead).
func (s *Server) fetchParentalAccount(ctx context.Context, accountID int) (*Account, *ParentalControls, error) {
	acc, err := s.store.FetchAccountByID(ctx, accountID)
	if err != nil || acc == nil {
		return nil, nil, err
	}
	ctl, err := s.store.FetchParentalControls(ctx, accountID)
	if err != nil {
		return nil, nil, err
	}
	return acc, ctl, nil
}

// checkParentPin verifies the presented PIN against the stored hash.
func checkParentPin(ctl *ParentalControls, pin string) bool {
	if ctl == nil {
		return false
	}
	return VerifyPassword(ctl.PinHash, pin)
}

// --- /parental/status ---

type parentalStatusResponse struct {
	Enabled                 bool   `json:"enabled"`
	RemainingSeconds        int    `json:"remaining_seconds"`
	Blocked                 bool   `json:"blocked"`
	BufferUntil             string `json:"buffer_until,omitempty"`
	ForceLogout             bool   `json:"force_logout,omitempty"`
	ChatAllowed             bool   `json:"chat_allowed"`
	VoiceAllowed            bool   `json:"voice_allowed"`
	DayMinutes              int    `json:"week_minutes"`
	WarningThresholdSeconds int    `json:"warning_threshold_seconds"`
	ExtendedUsedToday       bool   `json:"extended_used_today,omitempty"`
	Warning                 bool   `json:"warning,omitempty"`
}

// handleParentalStatus reports the effective server-side state. With a
// valid session the call also accrues play time and starts the grace
// buffer when the budget hits zero; without a session it is a pure
// read (login decision, info display).
func (s *Server) handleParentalStatus(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalStatus)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		SessionID string `json:"session_id"`
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
	sessionActive := false
	if req.SessionID != "" {
		if sess, err := s.store.ValidateSession(ctx, req.SessionID); err != nil {
			dbError(w, err, "session lookup failed")
			return
		} else if sess != nil && sess.AccountID == acc.ID {
			sessionActive = true
		}
	}
	st, err := s.computeParentalState(ctx, acc, sessionActive, true, time.Now())
	if err != nil {
		dbError(w, err, "parental status failed")
		return
	}
	resp := parentalStatusResponse{
		Enabled:                 st.Enabled,
		ChatAllowed:             true,
		VoiceAllowed:            true,
		WarningThresholdSeconds: 1800,
	}
	if st.Enabled {
		resp.RemainingSeconds = st.RemainingSeconds
		resp.Blocked = st.Blocked
		resp.ForceLogout = st.ForceLogout
		resp.ChatAllowed = st.ChatAllowed
		resp.VoiceAllowed = st.VoiceAllowed
		resp.DayMinutes = st.DayMinutes
		resp.WarningThresholdSeconds = st.WarningSeconds
		resp.ExtendedUsedToday = st.ExtendedUsedToday
		resp.Warning = st.Warning
		if st.BufferUntil != nil {
			resp.BufferUntil = st.BufferUntil.Format(time.RFC3339)
		}
	}
	writeJSON(w, http.StatusOK, resp)
}

// --- /parental/setup ---

type parentalWeekJSON struct {
	Monday    int `json:"monday_minutes"`
	Tuesday   int `json:"tuesday_minutes"`
	Wednesday int `json:"wednesday_minutes"`
	Thursday  int `json:"thursday_minutes"`
	Friday    int `json:"friday_minutes"`
	Saturday  int `json:"saturday_minutes"`
	Sunday    int `json:"sunday_minutes"`
}

func (j parentalWeekJSON) array() [7]int {
	return [7]int{j.Monday, j.Tuesday, j.Wednesday, j.Thursday, j.Friday, j.Saturday, j.Sunday}
}

// handleParentalSetup creates the control row, stores the PIN hash and
// enables the account flag. The parent e-mail is optional.
func (s *Server) handleParentalSetup(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID      int    `json:"account_id"`
		ParentPIN      string `json:"parent_pin"`
		ParentEmail    string `json:"parent_email"`
		ChatEnabled    bool   `json:"chat_enabled"`
		VoiceEnabled   bool   `json:"voice_enabled"`
		WarningMinutes int    `json:"warning_minutes"`
		parentalWeekJSON
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if !validParentPin(req.ParentPIN) {
		writeError(w, http.StatusBadRequest, "parent_pin must be 4-16 digits")
		return
	}
	week := req.array()
	if !validWeek(week) {
		writeError(w, http.StatusBadRequest, "week minutes must be 0-1440")
		return
	}
	if req.WarningMinutes < 0 || req.WarningMinutes > 1440 {
		writeError(w, http.StatusBadRequest, "warning_minutes must be 0-1440")
		return
	}
	email := trimSpace(req.ParentEmail)
	if email != "" && !validEmail(email) {
		writeError(w, http.StatusBadRequest, "invalid parent e-mail")
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
	if cur, err := s.store.FetchParentalControls(ctx, req.AccountID); err != nil {
		dbError(w, err, "parental lookup failed")
		return
	} else if cur != nil {
		writeError(w, http.StatusConflict, "parental control already set up")
		return
	}
	pinHash, err := HashPasswordSalt(req.ParentPIN, s.cfg.ArgonTime, s.cfg.ArgonMemory, s.cfg.ArgonThreads)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "pin hash failed")
		return
	}
	var emailEnc []byte
	if email != "" {
		emailEnc, err = aesGCMEncrypt(s.cfg.EncryptionKey, []byte(normalizeEmail(email)))
		if err != nil {
			writeError(w, http.StatusInternalServerError, "e-mail encryption failed")
			return
		}
	}
	ctl := &ParentalControls{
		AccountID: req.AccountID, PinHash: pinHash, ParentEmailEnc: emailEnc,
		Week: week, ChatEnabled: req.ChatEnabled, VoiceEnabled: req.VoiceEnabled,
		WarningMinutes: req.WarningMinutes,
	}
	if err := s.store.CreateParentalControl(ctx, ctl); err != nil {
		if isDuplicateKey(err) {
			writeError(w, http.StatusConflict, "parental control already set up")
			return
		}
		dbError(w, err, "parental setup failed")
		return
	}
	if err := s.store.SetParentalEnabled(ctx, req.AccountID, true); err != nil {
		dbError(w, err, "parental setup failed")
		return
	}
	s.recordEvent(ctx, eventParentalSetup, req.AccountID)
	s.notifyParental(ctx, req.AccountID, eventParentalSetup, "parental_control", "disabled", "enabled", emailEnc)
	writeJSON(w, http.StatusCreated, map[string]bool{"enabled": true})
}

// --- /parental/update ---

// handleParentalUpdate changes rules and/or the parent e-mail (PIN
// required). parent_email is tri-state: absent = keep, "" = remove,
// value = set.
func (s *Server) handleParentalUpdate(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID      int     `json:"account_id"`
		ParentPIN      string  `json:"parent_pin"`
		ParentEmail    *string `json:"parent_email"`
		ChatEnabled    bool    `json:"chat_enabled"`
		VoiceEnabled   bool    `json:"voice_enabled"`
		WarningMinutes int     `json:"warning_minutes"`
		parentalWeekJSON
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	week := req.array()
	if !validWeek(week) {
		writeError(w, http.StatusBadRequest, "week minutes must be 0-1440")
		return
	}
	if req.WarningMinutes < 0 || req.WarningMinutes > 1440 {
		writeError(w, http.StatusBadRequest, "warning_minutes must be 0-1440")
		return
	}
	var newEmail *string
	if req.ParentEmail != nil {
		e := trimSpace(*req.ParentEmail)
		if e != "" && !validEmail(e) {
			writeError(w, http.StatusBadRequest, "invalid parent e-mail")
			return
		}
		newEmail = &e
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	_, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.ParentPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	oldWeek := ctl.Week
	oldChat, oldVoice, oldWarn := ctl.ChatEnabled, ctl.VoiceEnabled, ctl.WarningMinutes
	var oldEmail string
	if len(ctl.ParentEmailEnc) > 0 {
		if plain, err := aesGCMDecrypt(s.cfg.EncryptionKey, ctl.ParentEmailEnc); err == nil {
			oldEmail = string(plain)
		}
	}
	ctl.Week = week
	ctl.ChatEnabled = req.ChatEnabled
	ctl.VoiceEnabled = req.VoiceEnabled
	ctl.WarningMinutes = req.WarningMinutes
	if err := s.store.UpdateParentalControls(ctx, ctl); err != nil {
		dbError(w, err, "parental update failed")
		return
	}
	// E-mail tri-state.
	recipient := ctl.ParentEmailEnc
	if newEmail != nil {
		if *newEmail == "" {
			// Removal: the previous address gets the receipt.
			if err := s.store.SetParentEmailEnc(ctx, req.AccountID, nil); err != nil {
				dbError(w, err, "parental update failed")
				return
			}
			s.notifyParental(ctx, req.AccountID, eventParentalEmailRemoved, "parent_email", oldEmail, "removed", recipient)
		} else {
			enc, err := aesGCMEncrypt(s.cfg.EncryptionKey, []byte(normalizeEmail(*newEmail)))
			if err != nil {
				writeError(w, http.StatusInternalServerError, "e-mail encryption failed")
				return
			}
			if err := s.store.SetParentEmailEnc(ctx, req.AccountID, enc); err != nil {
				dbError(w, err, "parental update failed")
				return
			}
			if normalizeEmail(*newEmail) != oldEmail {
				if oldEmail == "" {
					recipient = enc
				}
				s.notifyParental(ctx, req.AccountID, eventParentalEmailChanged, "parent_email", oldEmail, normalizeEmail(*newEmail), recipient)
			}
			recipient = enc
		}
	}
	// Per-field receipts for changed rules.
	names := [7]string{"monday_minutes", "tuesday_minutes", "wednesday_minutes", "thursday_minutes", "friday_minutes", "saturday_minutes", "sunday_minutes"}
	for i, n := range names {
		if oldWeek[i] != week[i] {
			s.notifyParental(ctx, req.AccountID, eventParentalSettings, n,
				itoa(oldWeek[i]), itoa(week[i]), recipient)
		}
	}
	if oldChat != req.ChatEnabled {
		s.notifyParental(ctx, req.AccountID, eventParentalSettings, "chat_enabled",
			boolStr(oldChat), boolStr(req.ChatEnabled), recipient)
	}
	if oldVoice != req.VoiceEnabled {
		s.notifyParental(ctx, req.AccountID, eventParentalSettings, "voice_enabled",
			boolStr(oldVoice), boolStr(req.VoiceEnabled), recipient)
	}
	if oldWarn != req.WarningMinutes {
		s.notifyParental(ctx, req.AccountID, eventParentalSettings, "warning_minutes",
			itoa(oldWarn), itoa(req.WarningMinutes), recipient)
	}
	s.recordEvent(ctx, eventParentalSettings, req.AccountID)
	writeJSON(w, http.StatusOK, map[string]bool{"updated": true})
}

// --- /parental/remove ---

// handleParentalRemove deletes the whole control (PIN required). The
// previous parent e-mail gets one final receipt.
func (s *Server) handleParentalRemove(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		ParentPIN string `json:"parent_pin"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	_, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.ParentPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	recipient := ctl.ParentEmailEnc
	if err := s.store.DeleteParentalControls(ctx, req.AccountID); err != nil {
		dbError(w, err, "parental remove failed")
		return
	}
	if err := s.store.SetParentalEnabled(ctx, req.AccountID, false); err != nil {
		dbError(w, err, "parental remove failed")
		return
	}
	s.recordEvent(ctx, eventParentalRemoved, req.AccountID)
	s.notifyParental(ctx, req.AccountID, eventParentalRemoved, "parental_control", "enabled", "removed", recipient)
	writeJSON(w, http.StatusOK, map[string]bool{"removed": true})
}

// --- /parental/pin/change ---

// handleParentalPinChange rotates the parent PIN. The receipt never
// contains a PIN.
func (s *Server) handleParentalPinChange(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		OldPIN    string `json:"old_pin"`
		NewPIN    string `json:"new_pin"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if !validParentPin(req.NewPIN) {
		writeError(w, http.StatusBadRequest, "new_pin must be 4-16 digits")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	_, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.OldPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	hash, err := HashPasswordSalt(req.NewPIN, s.cfg.ArgonTime, s.cfg.ArgonMemory, s.cfg.ArgonThreads)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "pin hash failed")
		return
	}
	if err := s.store.SetParentPinHash(ctx, req.AccountID, hash); err != nil {
		dbError(w, err, "pin change failed")
		return
	}
	s.recordEvent(ctx, eventParentalPinChanged, req.AccountID)
	s.notifyParental(ctx, req.AccountID, eventParentalPinChanged, "parent_pin", "****", "****", ctl.ParentEmailEnc)
	writeJSON(w, http.StatusOK, map[string]bool{"changed": true})
}

// --- /parental/pin/verify + /parental/extension ---

type parentalPinVerifyResponse struct {
	Valid bool `json:"valid"`
}

// handleParentalPinVerify answers the in-game parent panel: is the PIN
// correct? Shape stays {valid:false} when no control exists.
func (s *Server) handleParentalPinVerify(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalPin)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		ParentPIN string `json:"parent_pin"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	ctl, err := s.store.FetchParentalControls(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	writeJSON(w, http.StatusOK, parentalPinVerifyResponse{Valid: checkParentPin(ctl, req.ParentPIN)})
}

type parentalExtensionResponse struct {
	Granted           bool `json:"granted"`
	RemainingSeconds  int  `json:"remaining_seconds"`
	ExtendedUsedToday bool `json:"extended_used_today"`
}

// handleParentalExtension grants the once-per-day +1h after PIN entry.
// A second call the same day is 409 extension_used.
func (s *Server) handleParentalExtension(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalPin)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		ParentPIN string `json:"parent_pin"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	acc, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if acc == nil || ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.ParentPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	now := time.Now()
	if err := s.store.UseParentalExtension(ctx, req.AccountID, parentalDayKey(now), now); err != nil {
		if err == ErrExtensionUsed {
			writeError(w, http.StatusConflict, "extension already used today")
			return
		}
		dbError(w, err, "extension failed")
		return
	}
	st, err := s.computeParentalState(ctx, acc, false, false, now)
	if err != nil {
		dbError(w, err, "parental status failed")
		return
	}
	writeJSON(w, http.StatusOK, parentalExtensionResponse{
		Granted: true, RemainingSeconds: st.RemainingSeconds, ExtendedUsedToday: true,
	})
}

// --- periods ---

type parentalPeriodEntry struct {
	ID    int    `json:"id"`
	Start string `json:"starts_at"`
	End   string `json:"ends_at"`
	parentalWeekJSON
}

func periodEntry(p ParentalPeriod) parentalPeriodEntry {
	return parentalPeriodEntry{
		ID: p.ID, Start: parentalDayString(p.Start), End: parentalDayString(p.End),
		parentalWeekJSON: parentalWeekJSON{
			Monday: p.Week[0], Tuesday: p.Week[1], Wednesday: p.Week[2],
			Thursday: p.Week[3], Friday: p.Week[4], Saturday: p.Week[5], Sunday: p.Week[6],
		},
	}
}

// handleParentalPeriods lists the special periods of the account.
func (s *Server) handleParentalPeriods(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
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
	periods, err := s.store.ListParentalPeriods(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "period lookup failed")
		return
	}
	out := []parentalPeriodEntry{}
	for _, p := range periods {
		out = append(out, periodEntry(p))
	}
	writeJSON(w, http.StatusOK, map[string]any{"periods": out})
}

// handleParentalPeriodAdd adds one special period. Active periods must
// not overlap, so no priority logic between two active periods is
// ever needed.
func (s *Server) handleParentalPeriodAdd(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		ParentPIN string `json:"parent_pin"`
		StartsAt  string `json:"starts_at"`
		EndsAt    string `json:"ends_at"`
		parentalWeekJSON
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	week := req.array()
	if !validWeek(week) {
		writeError(w, http.StatusBadRequest, "week minutes must be 0-1440")
		return
	}
	start, okStart := parseParentalDate(req.StartsAt)
	end, okEnd := parseParentalDate(req.EndsAt)
	if !okStart || !okEnd || end.Before(start) || end.Sub(start) > 730*24*time.Hour {
		writeError(w, http.StatusBadRequest, "invalid period range (YYYY-MM-DD, max 730 days)")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	_, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.ParentPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	existing, err := s.store.ListParentalPeriods(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "period lookup failed")
		return
	}
	for _, p := range existing {
		s0, s1 := parentalDayString(p.Start), parentalDayString(p.End)
		if d0, d1 := parentalDayString(start), parentalDayString(end); d1 >= s0 && d0 <= s1 {
			writeError(w, http.StatusConflict, "period overlaps an existing period")
			return
		}
	}
	id, err := s.store.AddParentalPeriod(ctx, &ParentalPeriod{AccountID: req.AccountID, Start: start, End: end, Week: week})
	if err != nil {
		dbError(w, err, "period add failed")
		return
	}
	s.notifyParental(ctx, req.AccountID, eventParentalPeriod, "period", "-",
		req.StartsAt+".."+req.EndsAt, ctl.ParentEmailEnc)
	writeJSON(w, http.StatusCreated, map[string]any{"id": id})
}

// handleParentalPeriodRemove deletes one special period by id.
func (s *Server) handleParentalPeriodRemove(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		ParentPIN string `json:"parent_pin"`
		ID        int    `json:"id"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	_, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.ParentPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	if err := s.store.DeleteParentalPeriod(ctx, req.AccountID, req.ID); err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusNotFound, "period not found")
			return
		}
		dbError(w, err, "period remove failed")
		return
	}
	s.notifyParental(ctx, req.AccountID, eventParentalPeriod, "period", "set", "removed", ctl.ParentEmailEnc)
	writeJSON(w, http.StatusOK, map[string]bool{"removed": true})
}

// --- exceptions ---

// handleParentalExceptionAdd sets the day exception: either extra
// minutes on top of the resolved limit or a full override
// (0 = unlimited). Exactly one of the two must be given.
func (s *Server) handleParentalExceptionAdd(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID       int    `json:"account_id"`
		ParentPIN       string `json:"parent_pin"`
		Date            string `json:"date"`
		ExtraMinutes    *int   `json:"extra_minutes"`
		OverrideMinutes *int   `json:"override_minutes"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	day, okDay := parseParentalDate(req.Date)
	if !okDay {
		writeError(w, http.StatusBadRequest, "date must be YYYY-MM-DD")
		return
	}
	if (req.ExtraMinutes == nil) == (req.OverrideMinutes == nil) {
		writeError(w, http.StatusBadRequest, "exactly one of extra_minutes / override_minutes required")
		return
	}
	if req.ExtraMinutes != nil && (*req.ExtraMinutes < 1 || *req.ExtraMinutes > 1440) {
		writeError(w, http.StatusBadRequest, "extra_minutes must be 1-1440")
		return
	}
	if req.OverrideMinutes != nil && (*req.OverrideMinutes < 0 || *req.OverrideMinutes > 1440) {
		writeError(w, http.StatusBadRequest, "override_minutes must be 0-1440")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	_, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.ParentPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	id, err := s.store.SaveParentalException(ctx, &ParentalException{
		AccountID: req.AccountID, Date: day,
		ExtraMinutes: req.ExtraMinutes, OverrideMinutes: req.OverrideMinutes,
	})
	if err != nil {
		dbError(w, err, "exception save failed")
		return
	}
	desc := "extra+" + itoa(intOrZero(req.ExtraMinutes))
	if req.OverrideMinutes != nil {
		desc = "override=" + itoa(*req.OverrideMinutes)
	}
	s.notifyParental(ctx, req.AccountID, eventParentalException, "exception:"+req.Date, "-", desc, ctl.ParentEmailEnc)
	writeJSON(w, http.StatusCreated, map[string]any{"id": id})
}

// handleParentalExceptionRemove deletes the day exception.
func (s *Server) handleParentalExceptionRemove(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalManage)
	if !ok {
		return
	}
	var req struct {
		AccountID int    `json:"account_id"`
		ParentPIN string `json:"parent_pin"`
		Date      string `json:"date"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	day, okDay := parseParentalDate(req.Date)
	if !okDay {
		writeError(w, http.StatusBadRequest, "date must be YYYY-MM-DD")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	_, ctl, err := s.fetchParentalAccount(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "parental lookup failed")
		return
	}
	if ctl == nil {
		writeError(w, http.StatusNotFound, "parental control not found")
		return
	}
	if !checkParentPin(ctl, req.ParentPIN) {
		writeError(w, http.StatusUnauthorized, "parent pin does not match")
		return
	}
	if err := s.store.DeleteParentalException(ctx, req.AccountID, day); err != nil {
		if err == sql.ErrNoRows {
			writeError(w, http.StatusNotFound, "exception not found")
			return
		}
		dbError(w, err, "exception remove failed")
		return
	}
	s.notifyParental(ctx, req.AccountID, eventParentalException, "exception:"+req.Date, "set", "removed", ctl.ParentEmailEnc)
	writeJSON(w, http.StatusOK, map[string]bool{"removed": true})
}

// --- notifications ---

type parentalNotificationEntry struct {
	ID        int    `json:"id"`
	EventType string `json:"event_type"`
	Setting   string `json:"setting_name"`
	OldValue  string `json:"old_value"`
	NewValue  string `json:"new_value"`
	CreatedAt string `json:"created_at"`
}

// handleParentalNotifications lists pending (undelivered) receipts.
// No addresses are included here; the deliver call returns them once.
func (s *Server) handleParentalNotifications(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalNotify)
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
	if acc, err := s.store.FetchAccountByID(ctx, req.AccountID); err != nil {
		dbError(w, err, "account lookup failed")
		return
	} else if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	pending, err := s.store.ListPendingParentalNotifications(ctx, req.AccountID)
	if err != nil {
		dbError(w, err, "notification lookup failed")
		return
	}
	out := []parentalNotificationEntry{}
	for _, n := range pending {
		out = append(out, parentalNotificationEntry{
			ID: n.ID, EventType: n.EventType, Setting: n.SettingName,
			OldValue: n.OldValue, NewValue: n.NewValue,
			CreatedAt: n.CreatedAt.Format(time.RFC3339),
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{"notifications": out})
}

type parentalDeliveredEntry struct {
	ID    int    `json:"id"`
	Email string `json:"email"`
}

// handleParentalNotificationsDeliver marks receipts delivered and
// returns each affected address exactly once, so the calling service
// can send it. Afterwards the plaintext address is never returned
// again for that receipt.
func (s *Server) handleParentalNotificationsDeliver(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permParentalNotify)
	if !ok {
		return
	}
	var req struct {
		AccountID       int   `json:"account_id"`
		NotificationIDs []int `json:"notification_ids"`
	}
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if len(req.NotificationIDs) == 0 || len(req.NotificationIDs) > 100 {
		writeError(w, http.StatusBadRequest, "notification_ids must list 1-100 ids")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), verifyDBTimeout)
	defer cancel()
	if acc, err := s.store.FetchAccountByID(ctx, req.AccountID); err != nil {
		dbError(w, err, "account lookup failed")
		return
	} else if acc == nil {
		writeError(w, http.StatusNotFound, "account not found")
		return
	}
	marked, err := s.store.MarkParentalNotificationsDelivered(ctx, req.AccountID, req.NotificationIDs, time.Now())
	if err != nil {
		dbError(w, err, "notification deliver failed")
		return
	}
	out := []parentalDeliveredEntry{}
	for _, n := range marked {
		email := ""
		if len(n.RecipientEmailEnc) > 0 {
			if plain, err := aesGCMDecrypt(s.cfg.EncryptionKey, n.RecipientEmailEnc); err == nil {
				email = string(plain)
			}
		}
		out = append(out, parentalDeliveredEntry{ID: n.ID, Email: email})
	}
	writeJSON(w, http.StatusOK, map[string]any{"delivered": out})
}

// --- small helpers ---

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	neg := n < 0
	if neg {
		n = -n
	}
	var buf [20]byte
	i := len(buf)
	for n > 0 {
		i--
		buf[i] = byte('0' + n%10)
		n /= 10
	}
	if neg {
		i--
		buf[i] = '-'
	}
	return string(buf[i:])
}

func boolStr(b bool) string {
	if b {
		return "true"
	}
	return "false"
}

func intOrZero(p *int) int {
	if p == nil {
		return 0
	}
	return *p
}
