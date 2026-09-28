package main

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"fmt"
	"sort"
	"sync"
	"time"
)

// fakeStore is the in-memory AuthStore used by the integration tests:
// same contract as sQLStore, no database, deterministic outcomes.
type fakeStore struct {
	mu         sync.Mutex
	accounts   map[int]*Account
	nextID     int
	byName     map[string]int
	byEmail    map[string]int
	sessions   map[string]Session
	handoffs   map[string]*Handoff
	recoveries map[string]*Recovery
	realms     []Realm
	worlds     map[int]*WorldServer
	perms      map[int]map[string]bool
	lastLogin  map[int]*time.Time
	// TOTP-2FA / devices / recovery codes / security events.
	trustedDevices map[int]map[string]TrustedDevice
	// recoverFailStage injects a RecoverPassword failure (0 = off).
	recoverFailStage int
	// opFailStage injects a failure at a numbered business step of one
	// composite 2FA operation (0/absent = off); see failStepLocked.
	opFailStage    map[string]int
	recoveryCodes  map[int]map[string]*RecoveryCode
	securityEvents map[int][]SecurityEvent
	nextDeviceID   int
	nextEventID    int
	// Parental control (mirrors the parental_* tables).
	parental      map[int]*ParentalControls
	periods       map[int][]*ParentalPeriod
	nextPeriodID  int
	exceptions    map[int]map[string]*ParentalException
	nextExcID     int
	usage         map[int]map[string]*ParentalDailyUsage
	notifications map[int][]*ParentalNotification
	nextNotifID   int
}

func newFakeStore() *fakeStore {
	return &fakeStore{
		accounts:       map[int]*Account{},
		nextID:         1,
		byName:         map[string]int{},
		byEmail:        map[string]int{},
		sessions:       map[string]Session{},
		handoffs:       map[string]*Handoff{},
		recoveries:     map[string]*Recovery{},
		worlds:         map[int]*WorldServer{},
		perms:          map[int]map[string]bool{},
		lastLogin:      map[int]*time.Time{},
		trustedDevices: map[int]map[string]TrustedDevice{},
		recoveryCodes:  map[int]map[string]*RecoveryCode{},
		securityEvents: map[int][]SecurityEvent{},
		nextDeviceID:   1,
		nextEventID:    1,
		opFailStage:    map[string]int{},
		parental:       map[int]*ParentalControls{},
		periods:        map[int][]*ParentalPeriod{},
		nextPeriodID:   1,
		exceptions:     map[int]map[string]*ParentalException{},
		nextExcID:      1,
		usage:          map[int]map[string]*ParentalDailyUsage{},
		notifications:  map[int][]*ParentalNotification{},
		nextNotifID:    1,
	}
}

var _ AuthStore = (*fakeStore)(nil)

func (f *fakeStore) FetchAccount(_ context.Context, column string, value interface{}) (*Account, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	var id int
	switch column {
	case "username":
		id = f.byName[value.(string)]
	case "email_lookup_hash":
		id = f.byEmail[string(value.([]byte))]
	default:
		return nil, fmt.Errorf("unsupported lookup column %q", column)
	}
	if id == 0 {
		return nil, nil
	}
	return f.accounts[id], nil
}

func (f *fakeStore) FetchAccountByID(_ context.Context, id int) (*Account, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if acc := f.accounts[id]; acc != nil {
		return acc, nil
	}
	return nil, nil
}

func (f *fakeStore) RegisterAccount(_ context.Context, username, passwordHash, emailEncrypted string, emailLookupHash []byte, ttl time.Duration) (int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if _, ok := f.byName[username]; ok {
		return 0, fmt.Errorf("Duplicate entry for username")
	}
	if _, ok := f.byEmail[string(emailLookupHash)]; ok {
		return 0, fmt.Errorf("Duplicate entry for email")
	}
	now := time.Now()
	acc := &Account{
		ID:           f.nextID,
		Username:     username,
		PasswordHash: passwordHash,
		BanUntil:     &now,
	}
	// ttl is the fresh account ban (mirrors SQL NOW()+INTERVAL)
	*acc.BanUntil = now.Add(ttl)
	f.accounts[acc.ID] = acc
	f.byName[username] = acc.ID
	f.byEmail[string(emailLookupHash)] = acc.ID
	f.nextID++
	return acc.ID, nil
}

func (f *fakeStore) GrantAccountPermission(_ context.Context, accountID int, permission string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.perms[accountID] == nil {
		f.perms[accountID] = map[string]bool{}
	}
	f.perms[accountID][permission] = true
	return nil
}

func (f *fakeStore) ListAccountPermissions(_ context.Context, accountID int) ([]string, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []string{}
	for p := range f.perms[accountID] {
		out = append(out, p)
	}
	sort.Strings(out)
	return out, nil
}

func (f *fakeStore) TouchLogin(_ context.Context, accountID int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	now := time.Now()
	f.lastLogin[accountID] = &now
	return nil
}

func (f *fakeStore) CreateSession(_ context.Context, accountID int, ttl time.Duration) (string, Session, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if _, ok := f.accounts[accountID]; !ok {
		return "", Session{}, sql.ErrNoRows
	}
	now, raw, err := randomToken()
	if err != nil {
		return "", Session{}, err
	}
	sess := Session{TokenHash: tokenHash(raw), AccountID: accountID, CreatedAt: now, ExpiresAt: now.Add(ttl)}
	f.sessions[sess.TokenHash] = sess
	return raw, sess, nil
}

func (f *fakeStore) ValidateSession(_ context.Context, rawToken string) (*Session, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	sess, ok := f.sessions[tokenHash(rawToken)]
	if !ok {
		return nil, nil
	}
	// Revoked wins over expiry, exactly like sessionStatusOf in store.go.
	if sess.RevokedAt != nil || sess.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	s := sess
	return &s, nil
}

// statusOfLocked mirrors sessionStatusOf (store.go). Caller holds f.mu.
func statusOfLocked(sess Session) SessionStatus {
	if sess.RevokedAt != nil {
		return SessionRevoked
	}
	if sess.ExpiresAt.Before(time.Now()) {
		return SessionExpired
	}
	return SessionValid
}

func (f *fakeStore) SessionStatusOf(_ context.Context, rawToken string) (SessionStatus, int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	sess, ok := f.sessions[tokenHash(rawToken)]
	if !ok {
		return SessionMissing, 0, nil
	}
	return statusOfLocked(sess), sess.AccountID, nil
}

// BatchSessionStatus preserves input order, like the real single-IN query.
func (f *fakeStore) BatchSessionStatus(_ context.Context, tokens []string) ([]SessionStatus, []int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	statuses := make([]SessionStatus, len(tokens))
	accountIDs := make([]int, len(tokens))
	for i, t := range tokens {
		statuses[i] = SessionMissing
		if sess, ok := f.sessions[tokenHash(t)]; ok {
			statuses[i] = statusOfLocked(sess)
			accountIDs[i] = sess.AccountID
		}
	}
	return statuses, accountIDs, nil
}

func (f *fakeStore) RevokeSession(_ context.Context, sessionID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	h := tokenHash(sessionID)
	if sess, ok := f.sessions[h]; ok && sess.RevokedAt == nil {
		now := time.Now()
		sess.RevokedAt = &now
		f.sessions[h] = sess
	}
	return nil
}

func (f *fakeStore) CreateHandoff(_ context.Context, accountID, realmID int, ttl time.Duration) (string, time.Time, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	now, raw, err := randomToken()
	if err != nil {
		return "", time.Time{}, err
	}
	exp := now.Add(ttl)
	f.handoffs[tokenHash(raw)] = &Handoff{TokenHash: tokenHash(raw), AccountID: accountID, RealmID: realmID, CreatedAt: now, ExpiresAt: exp}
	return raw, exp, nil
}

func (f *fakeStore) ValidateHandoff(_ context.Context, rawToken string) (*Handoff, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	h, ok := f.handoffs[tokenHash(rawToken)]
	if !ok || h.UsedAt != nil {
		return nil, nil
	}
	if h.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	c := *h
	return &c, nil
}

func (f *fakeStore) UseHandoff(_ context.Context, rawToken string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	h, ok := f.handoffs[tokenHash(rawToken)]
	if !ok || h.UsedAt != nil {
		return sql.ErrNoRows
	}
	now := time.Now()
	h.UsedAt = &now
	return nil
}

func (f *fakeStore) ListRealms(_ context.Context) ([]Realm, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]Realm, len(f.realms))
	copy(out, f.realms)
	return out, nil
}

func (f *fakeStore) FetchWorldServer(_ context.Context, id int) (*WorldServer, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	ws, ok := f.worlds[id]
	if !ok {
		return nil, nil
	}
	c := *ws
	return &c, nil
}

func (f *fakeStore) AuthenticateWorld(_ context.Context, id int, credential string) (*WorldServer, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	ws, ok := f.worlds[id]
	if !ok || ws.Credential == "" || !ws.Enabled || !hmacEqual(ws.Credential, credential) {
		return nil, sql.ErrNoRows
	}
	c := *ws
	return &c, nil
}

func (f *fakeStore) RecordHeartbeat(_ context.Context, id int, version string, currentPlayers int, ok bool) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	ws, ok := f.worlds[id]
	if !ok {
		return sql.ErrNoRows
	}
	ws.Version = version
	ws.CurrentPlayers = currentPlayers
	if ok {
		ws.Status = "online"
	} else {
		ws.Status = "offline"
	}
	now := time.Now()
	ws.LastHeartbeat = &now
	return nil
}

func (f *fakeStore) UpdatePassword(_ context.Context, accountID int, newPasswordHash string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc, ok := f.accounts[accountID]
	if !ok {
		return sql.ErrNoRows
	}
	acc.PasswordHash = newPasswordHash
	return nil
}

func (f *fakeStore) CreateRecoveryToken(_ context.Context, accountID int, ttl time.Duration) (string, Recovery, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	now, raw, err := randomToken()
	if err != nil {
		return "", Recovery{}, err
	}
	rec := Recovery{TokenHash: tokenHash(raw), AccountID: accountID, CreatedAt: now, ExpiresAt: now.Add(ttl)}
	f.recoveries[rec.TokenHash] = &rec
	return raw, rec, nil
}

func (f *fakeStore) ValidateRecovery(_ context.Context, rawToken string) (*Recovery, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	rec, ok := f.recoveries[tokenHash(rawToken)]
	if !ok || rec.UsedAt != nil {
		return nil, nil
	}
	if rec.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	c := *rec
	return &c, nil
}

// recoverFailAt injects a failure into RecoverPassword AFTER the password was
// changed but BEFORE the commit, to prove the transaction rolls back
// completely (AUTH-02b). 0 = no injection. Values 1..4 mark the stage.
func (f *fakeStore) recoverFailAt(stage int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.recoverFailStage = stage
}

func (f *fakeStore) RecoverPassword(_ context.Context, accountID int, newPasswordHash string, rawRecoveryToken string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	rec, ok := f.recoveries[tokenHash(rawRecoveryToken)]
	if !ok || rec.UsedAt != nil {
		return sql.ErrNoRows
	}
	acc, ok := f.accounts[accountID]
	if !ok {
		return sql.ErrNoRows
	}
	// Snapshot for the rollback assertion: the fake mutates a copy and only
	// writes it back after every step succeeded, which is what a real
	// transaction commit does.
	oldHash := acc.PasswordHash
	oldBan := acc.BanUntil
	oldSessions := map[string]Session{}
	for h, s := range f.sessions {
		oldSessions[h] = s
	}
	oldDevices := f.trustedDevices[accountID]
	eventsBefore := len(f.securityEvents[accountID])
	fail := func(stage int) error {
		if f.recoverFailStage == stage {
			// Undo everything, as tx.Rollback() would.
			acc.PasswordHash = oldHash
			acc.BanUntil = oldBan
			for h, s := range oldSessions {
				f.sessions[h] = s
			}
			f.trustedDevices[accountID] = oldDevices
			f.securityEvents[accountID] = f.securityEvents[accountID][:eventsBefore]
			return errors.New("injected recovery failure")
		}
		return nil
	}
	acc.PasswordHash = newPasswordHash
	acc.BanUntil = nil
	now := time.Now()
	if err := fail(1); err != nil {
		return err
	}
	// stage 1: sessions marked
	for h, s := range f.sessions {
		if s.AccountID == accountID && s.RevokedAt == nil {
			s.RevokedAt = &now
			f.sessions[h] = s
		}
	}
	if err := fail(2); err != nil {
		return err
	}
	// stage 2: trusted devices removed
	delete(f.trustedDevices, accountID)
	if err := fail(3); err != nil {
		return err
	}
	// stage 3: security event written
	f.recordEventLocked(accountID, eventPasswordReset)
	if err := fail(4); err != nil {
		return err
	}
	// stage 4: recovery token consumed (last step, as in the SQL)
	rec.UsedAt = &now
	return nil
}

// --- TOTP-2FA / trusted devices / recovery codes / security events ---

func (f *fakeStore) SetTwoFactorEnabled(_ context.Context, accountID int, enabled bool) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return sql.ErrNoRows
	}
	acc.TwoFactorEnabled = enabled
	return nil
}

func (f *fakeStore) SaveTwoFactorSecret(_ context.Context, accountID int, encryptedSecret []byte) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return sql.ErrNoRows
	}
	s := make([]byte, len(encryptedSecret))
	copy(s, encryptedSecret)
	acc.TwoFactorSecret = s
	acc.LastTOTPCounter = nil
	acc.LastTOTPAt = nil
	return nil
}

func (f *fakeStore) ClearTwoFactorSecret(_ context.Context, accountID int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return sql.ErrNoRows
	}
	acc.TwoFactorSecret = nil
	acc.LastTOTPCounter = nil
	acc.LastTOTPAt = nil
	return nil
}

func (f *fakeStore) SetLastTOTP(_ context.Context, accountID int, counter int, at time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return sql.ErrNoRows
	}
	acc.LastTOTPCounter = &counter
	ts := at
	acc.LastTOTPAt = &ts
	return nil
}

// ChangePasswordRevokeAll mirrors the SQL transaction: new password,
// all sessions revoked, all trusted devices revoked, security event.
func (f *fakeStore) ChangePasswordRevokeAll(_ context.Context, accountID int, newPasswordHash string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return sql.ErrNoRows
	}
	acc.PasswordHash = newPasswordHash
	now := time.Now()
	for h, s := range f.sessions {
		if s.AccountID == accountID && s.RevokedAt == nil {
			s.RevokedAt = &now
			f.sessions[h] = s
		}
	}
	delete(f.trustedDevices, accountID)
	f.recordEventLocked(accountID, eventPasswordChanged)
	return nil
}

// countActiveDevicesLocked counts not-yet-expired devices. Caller must
// hold f.mu.
func (f *fakeStore) countActiveDevicesLocked(accountID int, now time.Time) int {
	n := 0
	for _, d := range f.trustedDevices[accountID] {
		if trustedDeviceActive(d.LastUsedAt, now) {
			n++
		}
	}
	return n
}

func (f *fakeStore) CountTrustedDevices(_ context.Context, accountID int) (int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.countActiveDevicesLocked(accountID, time.Now()), nil
}

func (f *fakeStore) AddTrustedDevice(_ context.Context, accountID int, rawToken, label string) (TrustedDevice, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.accounts[accountID] == nil {
		return TrustedDevice{}, sql.ErrNoRows
	}
	now := time.Now()
	// Mirror the SQL transaction: purge expired rows (their unique
	// token_hash key would otherwise collide on re-presentation) and
	// count only active devices against the limit.
	for h, d := range f.trustedDevices[accountID] {
		if !trustedDeviceActive(d.LastUsedAt, now) {
			delete(f.trustedDevices[accountID], h)
		}
	}
	if f.countActiveDevicesLocked(accountID, now) >= maxTrustedDevices {
		return TrustedDevice{}, ErrMaxDevices
	}
	d := TrustedDevice{
		ID:          f.nextDeviceID,
		AccountID:   accountID,
		TokenHash:   tokenHash(rawToken),
		Label:       label,
		ConfirmedAt: now,
		CreatedAt:   now,
		LastUsedAt:  now,
	}
	f.nextDeviceID++
	if f.trustedDevices[accountID] == nil {
		f.trustedDevices[accountID] = map[string]TrustedDevice{}
	}
	f.trustedDevices[accountID][d.TokenHash] = d
	f.recordEventLocked(accountID, eventDeviceConfirmed)
	return d, nil
}

func (f *fakeStore) HasTrustedDevice(_ context.Context, accountID int, rawToken string) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	d, ok := f.trustedDevices[accountID][tokenHash(rawToken)]
	return ok && trustedDeviceActive(d.LastUsedAt, time.Now()), nil
}

func (f *fakeStore) TouchTrustedDevice(_ context.Context, accountID int, rawToken string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	devs := f.trustedDevices[accountID]
	h := tokenHash(rawToken)
	d, ok := devs[h]
	if !ok {
		return sql.ErrNoRows
	}
	now := time.Now()
	d.LastUsedAt = now
	devs[h] = d
	return nil
}

func (f *fakeStore) RevokeTrustedDevice(_ context.Context, accountID int, rawToken string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	snap := f.snapshotLocked(accountID)
	devs := f.trustedDevices[accountID]
	h := tokenHash(rawToken)
	if devs == nil {
		return sql.ErrNoRows
	}
	if _, ok := devs[h]; !ok {
		return sql.ErrNoRows
	}
	delete(devs, h)
	if err := f.failStepLocked(opRevokeDevice, accountID, snap, 1); err != nil {
		return err
	}
	f.recordEventLocked(accountID, eventDeviceRevoked)
	if err := f.failStepLocked(opRevokeDevice, accountID, snap, 2); err != nil {
		return err
	}
	return nil
}

func (f *fakeStore) ListTrustedDevices(_ context.Context, accountID int) ([]TrustedDevice, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	devs := f.trustedDevices[accountID]
	out := make([]TrustedDevice, 0, len(devs))
	for _, d := range devs {
		c := d
		out = append(out, c)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out, nil
}

func (f *fakeStore) RevokeAllTrustedDevices(_ context.Context, accountID int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	delete(f.trustedDevices, accountID)
	return nil
}

func (f *fakeStore) SaveRecoveryCodes(_ context.Context, accountID int, codes []string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.accounts[accountID] == nil {
		return sql.ErrNoRows
	}
	store := f.recoveryCodes[accountID]
	if store == nil {
		store = map[string]*RecoveryCode{}
		f.recoveryCodes[accountID] = store
	}
	now := time.Now()
	for _, c := range codes {
		// Normalized hash ensures cosmetic variants collide, matching
		// the SQL STORE.
		h := hashRecoveryCode(c)
		rc := &RecoveryCode{TokenHash: h, AccountID: accountID, CreatedAt: now}
		store[h] = rc
	}
	return nil
}

func (f *fakeStore) UseRecoveryCode(_ context.Context, accountID int, rawCode string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	snap := f.snapshotLocked(accountID)
	store := f.recoveryCodes[accountID]
	h := hashRecoveryCode(rawCode)
	rc, ok := store[h]
	if !ok || rc.UsedAt != nil {
		return sql.ErrNoRows
	}
	now := time.Now()
	rc.UsedAt = &now
	if err := f.failStepLocked(opUseRecoveryCode, accountID, snap, 1); err != nil {
		return err
	}
	f.recordEventLocked(accountID, eventRecoveryCodeUsed)
	if err := f.failStepLocked(opUseRecoveryCode, accountID, snap, 2); err != nil {
		return err
	}
	return nil
}

func (f *fakeStore) ClearRecoveryCodes(_ context.Context, accountID int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	delete(f.recoveryCodes, accountID)
	return nil
}

// CountRecoveryCodes returns how many (unused) recovery codes the
// account still has.
func (f *fakeStore) CountRecoveryCodes(_ context.Context, accountID int) (int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	n := 0
	for _, rc := range f.recoveryCodes[accountID] {
		if rc.UsedAt == nil {
			n++
		}
	}
	return n, nil
}

// RevokeAllSessions MARKS every live session of the account (migration 014).
func (f *fakeStore) RevokeAllSessions(_ context.Context, accountID int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.accounts[accountID] == nil {
		return sql.ErrNoRows
	}
	now := time.Now()
	for h, s := range f.sessions {
		if s.AccountID == accountID && s.RevokedAt == nil {
			s.RevokedAt = &now
			f.sessions[h] = s
		}
	}
	return nil
}

// recordEventLocked appends a security event. Caller must hold f.mu.
func (f *fakeStore) recordEventLocked(accountID int, eventType string) {
	now := time.Now()
	aid := accountID
	e := SecurityEvent{ID: f.nextEventID, EventType: eventType, AccountID: &aid, CreatedAt: now}
	f.nextEventID++
	f.securityEvents[accountID] = append(f.securityEvents[accountID], e)
}

// --- P-34: transactional fake for the composite 2FA operations ---
//
// The fake used to be ATOMICER than the real store: it appended
// device_revoked and recovery_code_used inside the same method call, and
// recordEventLocked could not fail at all, which hid the production bug.
// The helpers below give the composite operations the same step order, the
// same conditional transitions and the same complete undo as the SQL store.

// txSnapshot captures the business state one composite operation may
// change, so a simulated rollback can undo all of it.
//
// nextEventID and nextDeviceID are deliberately NOT captured: an InnoDB
// AUTO_INCREMENT counter is not rolled back either, so the fake must not
// promise gapless event ids across a failure. Tests assert event COUNT,
// type, account and commit, never id continuity.
type txSnapshot struct {
	secret        []byte
	enabled       bool
	lastCounter   *int
	lastAt        *time.Time
	recoveryCodes map[string]*RecoveryCode
	devices       map[string]TrustedDevice
	sessions      map[string]Session
	events        []SecurityEvent
}

func (f *fakeStore) snapshotLocked(accountID int) txSnapshot {
	acc := f.accounts[accountID]
	snap := txSnapshot{
		recoveryCodes: map[string]*RecoveryCode{},
		devices:       map[string]TrustedDevice{},
		sessions:      map[string]Session{},
	}
	if acc != nil {
		snap.secret = append([]byte(nil), acc.TwoFactorSecret...)
		snap.enabled = acc.TwoFactorEnabled
		if acc.LastTOTPCounter != nil {
			c := *acc.LastTOTPCounter
			snap.lastCounter = &c
		}
		if acc.LastTOTPAt != nil {
			t := *acc.LastTOTPAt
			snap.lastAt = &t
		}
	}
	for h, rc := range f.recoveryCodes[accountID] {
		c := *rc
		snap.recoveryCodes[h] = &c
	}
	for h, d := range f.trustedDevices[accountID] {
		snap.devices[h] = d
	}
	for h, s := range f.sessions {
		if s.AccountID == accountID {
			snap.sessions[h] = s
		}
	}
	snap.events = append([]SecurityEvent(nil), f.securityEvents[accountID]...)
	return snap
}

// restoreLocked undoes everything since the snapshot, which is what
// tx.Rollback() does in the SQL store.
func (f *fakeStore) restoreLocked(accountID int, snap txSnapshot) {
	if acc := f.accounts[accountID]; acc != nil {
		acc.TwoFactorSecret = append([]byte(nil), snap.secret...)
		if acc.TwoFactorSecret != nil && len(acc.TwoFactorSecret) == 0 {
			acc.TwoFactorSecret = nil
		}
		acc.TwoFactorEnabled = snap.enabled
		acc.LastTOTPCounter = snap.lastCounter
		acc.LastTOTPAt = snap.lastAt
	}
	restored := map[string]*RecoveryCode{}
	for h, rc := range snap.recoveryCodes {
		restored[h] = rc
	}
	f.recoveryCodes[accountID] = restored
	devices := map[string]TrustedDevice{}
	for h, d := range snap.devices {
		devices[h] = d
	}
	f.trustedDevices[accountID] = devices
	for h, s := range f.sessions {
		if s.AccountID == accountID {
			if _, existed := snap.sessions[h]; !existed {
				delete(f.sessions, h)
			}
		}
	}
	for h, s := range snap.sessions {
		f.sessions[h] = s
	}
	f.securityEvents[accountID] = append([]SecurityEvent(nil), snap.events...)
}

// failStepLocked injects a failure at a numbered step of a composite
// operation and completes the simulated rollback. 0 = no injection.
func (f *fakeStore) failStepLocked(op string, accountID int, snap txSnapshot, stage int) error {
	if f.opFailStage[op] != stage {
		return nil
	}
	f.restoreLocked(accountID, snap)
	return fmt.Errorf("injected %s failure at stage %d", op, stage)
}

// failStageAt injects a failure at the given numbered step of op. The step
// numbers follow the order in store.go.
func (f *fakeStore) failStageAt(op string, stage int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.opFailStage[op] = stage
}

// failEventWriteAt injects a failure at the security-event write of op, so a
// test can prove that a failing event rolls the whole operation back.
func (f *fakeStore) failEventWriteAt(op string) {
	f.failStageAt(op, fakeEventStage[op])
}

// fakeEventStage maps an operation to the step number of its security-event
// write, mirroring the position of insertSecurityEventTx in store.go.
var fakeEventStage = map[string]int{
	opSetupTwoFactor:   4,
	opEnableTwoFactor:  2,
	opDisableTwoFactor: 5,
	opResetTwoFactor:   7,
	opUseRecoveryCode:  2,
	opRevokeDevice:     2,
}

// fakeStageCount is the number of business steps per operation, including
// the event write, so tests can loop over every step.
var fakeStageCount = map[string]int{
	opSetupTwoFactor:   4,
	opEnableTwoFactor:  2,
	opDisableTwoFactor: 5,
	opResetTwoFactor:   7,
	opUseRecoveryCode:  2,
	opRevokeDevice:     2,
}

// Operation names used by the injection helper. They match the operation=
// value the store logs.
const (
	opSetupTwoFactor   = "setup_two_factor"
	opEnableTwoFactor  = "enable_two_factor"
	opDisableTwoFactor = "disable_two_factor"
	opResetTwoFactor   = "reset_two_factor"
	opUseRecoveryCode  = "use_recovery_code"
	opRevokeDevice     = "revoke_trusted_device"
)

// SetupTwoFactor mirrors sQLStore.SetupTwoFactor: gate, secret, codes,
// event. The gate is decided inside the "transaction", never from a
// handler-side pre-read.
func (f *fakeStore) SetupTwoFactor(_ context.Context, accountID int, encryptedSecret []byte, codes []string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return sql.ErrNoRows
	}
	snap := f.snapshotLocked(accountID)
	// stage 1: conditional gate, only fires while 2FA is off
	if !acc.TwoFactorEnabled {
		acc.TwoFactorEnabled = true
	} else {
		f.restoreLocked(accountID, snap)
		return ErrTwoFactorAlreadySetUp
	}
	if err := f.failStepLocked(opSetupTwoFactor, accountID, snap, 1); err != nil {
		return err
	}
	// stage 2: secret
	acc.TwoFactorSecret = append([]byte(nil), encryptedSecret...)
	acc.LastTOTPCounter = nil
	acc.LastTOTPAt = nil
	if err := f.failStepLocked(opSetupTwoFactor, accountID, snap, 2); err != nil {
		return err
	}
	// stage 3: recovery codes
	store := map[string]*RecoveryCode{}
	now := time.Now()
	for _, c := range codes {
		store[hashRecoveryCode(c)] = &RecoveryCode{TokenHash: hashRecoveryCode(c), AccountID: accountID, CreatedAt: now}
	}
	f.recoveryCodes[accountID] = store
	if err := f.failStepLocked(opSetupTwoFactor, accountID, snap, 3); err != nil {
		return err
	}
	// stage 4: security event (last business step)
	f.recordEventLocked(accountID, eventTwoFactorEnabled)
	if err := f.failStepLocked(opSetupTwoFactor, accountID, snap, 4); err != nil {
		return err
	}
	return nil
}

// EnableTwoFactor decides the transition inside the transaction and writes
// no event on a no-op.
func (f *fakeStore) EnableTwoFactor(_ context.Context, accountID int) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return false, sql.ErrNoRows
	}
	snap := f.snapshotLocked(accountID)
	if acc.TwoFactorEnabled {
		return false, nil
	}
	acc.TwoFactorEnabled = true
	if err := f.failStepLocked(opEnableTwoFactor, accountID, snap, 1); err != nil {
		return false, err
	}
	f.recordEventLocked(accountID, eventTwoFactorEnabled)
	if err := f.failStepLocked(opEnableTwoFactor, accountID, snap, 2); err != nil {
		return false, err
	}
	return true, nil
}

// DisableTwoFactor decides the transition inside the transaction, then
// clears secret, codes and devices. Sessions deliberately stay active.
func (f *fakeStore) DisableTwoFactor(_ context.Context, accountID int) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return false, sql.ErrNoRows
	}
	snap := f.snapshotLocked(accountID)
	if !acc.TwoFactorEnabled {
		return false, nil
	}
	acc.TwoFactorEnabled = false
	if err := f.failStepLocked(opDisableTwoFactor, accountID, snap, 1); err != nil {
		return false, err
	}
	acc.TwoFactorSecret = nil
	acc.LastTOTPCounter = nil
	acc.LastTOTPAt = nil
	if err := f.failStepLocked(opDisableTwoFactor, accountID, snap, 2); err != nil {
		return false, err
	}
	delete(f.recoveryCodes, accountID)
	if err := f.failStepLocked(opDisableTwoFactor, accountID, snap, 3); err != nil {
		return false, err
	}
	delete(f.trustedDevices, accountID)
	if err := f.failStepLocked(opDisableTwoFactor, accountID, snap, 4); err != nil {
		return false, err
	}
	f.recordEventLocked(accountID, eventTwoFactorDisabled)
	if err := f.failStepLocked(opDisableTwoFactor, accountID, snap, 5); err != nil {
		return false, err
	}
	return true, nil
}

// ResetTwoFactor mirrors the SQL store step for step, including the
// compare-and-swap on the stored secret. Two requests that read the SAME
// stored secret cannot both commit: the loser gets changed=false, a nil
// error and has written nothing at all.
func (f *fakeStore) ResetTwoFactor(_ context.Context, accountID int, expectedSecret, newSecret []byte, codes []string) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	acc := f.accounts[accountID]
	if acc == nil {
		return false, sql.ErrNoRows
	}
	snap := f.snapshotLocked(accountID)
	// stage 1: compare-and-swap on the previously stored secret. bytes.Equal
	// plus a nil check is the Go equivalent of SQL's NULL-safe "<=>".
	storedMatches := (acc.TwoFactorSecret == nil && expectedSecret == nil) ||
		(acc.TwoFactorSecret != nil && expectedSecret != nil && bytes.Equal(acc.TwoFactorSecret, expectedSecret))
	if !storedMatches {
		return false, nil
	}
	acc.TwoFactorSecret = append([]byte(nil), newSecret...)
	acc.LastTOTPCounter = nil
	acc.LastTOTPAt = nil
	if err := f.failStepLocked(opResetTwoFactor, accountID, snap, 1); err != nil {
		return false, err
	}
	delete(f.recoveryCodes, accountID)
	if err := f.failStepLocked(opResetTwoFactor, accountID, snap, 2); err != nil {
		return false, err
	}
	store := map[string]*RecoveryCode{}
	now := time.Now()
	for _, c := range codes {
		store[hashRecoveryCode(c)] = &RecoveryCode{TokenHash: hashRecoveryCode(c), AccountID: accountID, CreatedAt: now}
	}
	f.recoveryCodes[accountID] = store
	if err := f.failStepLocked(opResetTwoFactor, accountID, snap, 3); err != nil {
		return false, err
	}
	acc.TwoFactorEnabled = true
	if err := f.failStepLocked(opResetTwoFactor, accountID, snap, 4); err != nil {
		return false, err
	}
	revokeNow := time.Now()
	for h, s := range f.sessions {
		if s.AccountID == accountID && s.RevokedAt == nil {
			s.RevokedAt = &revokeNow
			f.sessions[h] = s
		}
	}
	if err := f.failStepLocked(opResetTwoFactor, accountID, snap, 5); err != nil {
		return false, err
	}
	delete(f.trustedDevices, accountID)
	if err := f.failStepLocked(opResetTwoFactor, accountID, snap, 6); err != nil {
		return false, err
	}
	f.recordEventLocked(accountID, eventTwoFactorReset)
	if err := f.failStepLocked(opResetTwoFactor, accountID, snap, 7); err != nil {
		return false, err
	}
	return true, nil
}

func (f *fakeStore) RecordSecurityEvent(_ context.Context, eventType string, accountID *int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	aid := 0
	if accountID != nil {
		aid = *accountID
	}
	e := SecurityEvent{ID: f.nextEventID, EventType: eventType, AccountID: accountID, CreatedAt: time.Now()}
	f.nextEventID++
	f.securityEvents[aid] = append(f.securityEvents[aid], e)
	return nil
}

func (f *fakeStore) ListSecurityEvents(_ context.Context, accountID int) ([]SecurityEvent, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	evs := f.securityEvents[accountID]
	out := make([]SecurityEvent, len(evs))
	copy(out, evs)
	// newest first, like the SQL ORDER BY id DESC
	for i, j := 0, len(out)-1; i < j; i, j = i+1, j-1 {
		out[i], out[j] = out[j], out[i]
	}
	return out, nil
}

// --- parental control (in-memory mirror of the parental_* tables) ---

func copyBytes(b []byte) []byte {
	if b == nil {
		return nil
	}
	c := make([]byte, len(b))
	copy(c, b)
	return c
}

func (f *fakeStore) FetchParentalControls(_ context.Context, accountID int) (*ParentalControls, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if c := f.parental[accountID]; c != nil {
		cp := *c
		cp.ParentEmailEnc = copyBytes(c.ParentEmailEnc)
		return &cp, nil
	}
	return nil, nil
}

func (f *fakeStore) CreateParentalControl(_ context.Context, ctl *ParentalControls) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if _, ok := f.parental[ctl.AccountID]; ok {
		return fmt.Errorf("Duplicate entry for parental_controls")
	}
	cp := *ctl
	cp.ParentEmailEnc = copyBytes(ctl.ParentEmailEnc)
	f.parental[ctl.AccountID] = &cp
	return nil
}

func (f *fakeStore) UpdateParentalControls(_ context.Context, ctl *ParentalControls) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	cur, ok := f.parental[ctl.AccountID]
	if !ok {
		return sql.ErrNoRows
	}
	cur.Week = ctl.Week
	cur.ChatEnabled = ctl.ChatEnabled
	cur.VoiceEnabled = ctl.VoiceEnabled
	cur.WarningMinutes = ctl.WarningMinutes
	cur.UpdatedAt = time.Now()
	return nil
}

func (f *fakeStore) SetParentPinHash(_ context.Context, accountID int, hash string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	cur, ok := f.parental[accountID]
	if !ok {
		return sql.ErrNoRows
	}
	cur.PinHash = hash
	return nil
}

func (f *fakeStore) SetParentEmailEnc(_ context.Context, accountID int, enc []byte) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	cur, ok := f.parental[accountID]
	if !ok {
		return sql.ErrNoRows
	}
	cur.ParentEmailEnc = copyBytes(enc)
	return nil
}

func (f *fakeStore) SetParentalEnabled(_ context.Context, accountID int, enabled bool) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if acc := f.accounts[accountID]; acc != nil {
		acc.ParentalEnabled = enabled
	}
	return nil
}

func (f *fakeStore) DeleteParentalControls(_ context.Context, accountID int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	delete(f.parental, accountID)
	delete(f.periods, accountID)
	delete(f.exceptions, accountID)
	delete(f.usage, accountID)
	return nil
}

func (f *fakeStore) ListParentalPeriods(_ context.Context, accountID int) ([]ParentalPeriod, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []ParentalPeriod{}
	for _, p := range f.periods[accountID] {
		out = append(out, *p)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Start.Equal(out[j].Start) {
			return out[i].ID < out[j].ID
		}
		return out[i].Start.Before(out[j].Start)
	})
	return out, nil
}

func (f *fakeStore) AddParentalPeriod(_ context.Context, p *ParentalPeriod) (int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	cp := *p
	cp.ID = f.nextPeriodID
	f.nextPeriodID++
	f.periods[p.AccountID] = append(f.periods[p.AccountID], &cp)
	return cp.ID, nil
}

func (f *fakeStore) DeleteParentalPeriod(_ context.Context, accountID, id int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	list := f.periods[accountID]
	for i, p := range list {
		if p.ID == id {
			f.periods[accountID] = append(list[:i], list[i+1:]...)
			return nil
		}
	}
	return sql.ErrNoRows
}

func (f *fakeStore) GetActiveParentalPeriod(_ context.Context, accountID int, date time.Time) (*ParentalPeriod, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	// Compare calendar-day strings (like the SQL DATE comparison) so
	// parsed period bounds (UTC) match server-local poll days.
	day := parentalDayString(date)
	for _, p := range f.periods[accountID] {
		if s0, s1 := parentalDayString(p.Start), parentalDayString(p.End); day >= s0 && day <= s1 {
			cp := *p
			return &cp, nil
		}
	}
	return nil, nil
}

func (f *fakeStore) FetchParentalException(_ context.Context, accountID int, date time.Time) (*ParentalException, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if e := f.exceptions[accountID][parentalDayString(date)]; e != nil {
		cp := *e
		return &cp, nil
	}
	return nil, nil
}

func (f *fakeStore) SaveParentalException(_ context.Context, e *ParentalException) (int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	key := parentalDayString(e.Date)
	if f.exceptions[e.AccountID] == nil {
		f.exceptions[e.AccountID] = map[string]*ParentalException{}
	}
	if cur := f.exceptions[e.AccountID][key]; cur != nil {
		cur.ExtraMinutes = e.ExtraMinutes
		cur.OverrideMinutes = e.OverrideMinutes
		return cur.ID, nil
	}
	cp := *e
	cp.ID = f.nextExcID
	f.nextExcID++
	f.exceptions[e.AccountID][key] = &cp
	return cp.ID, nil
}

func (f *fakeStore) DeleteParentalException(_ context.Context, accountID int, date time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	key := parentalDayString(date)
	if f.exceptions[accountID] == nil || f.exceptions[accountID][key] == nil {
		return sql.ErrNoRows
	}
	delete(f.exceptions[accountID], key)
	return nil
}

func (f *fakeStore) FetchParentalUsage(_ context.Context, accountID int, date time.Time) (ParentalDailyUsage, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	u := ParentalDailyUsage{AccountID: accountID, Date: parentalDayKey(date)}
	if cur := f.usage[accountID][parentalDayString(date)]; cur != nil {
		u = *cur
	}
	return u, nil
}

func (f *fakeStore) AddParentalUsageSeconds(_ context.Context, accountID int, date time.Time, seconds int, lastPolled time.Time, _ int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	key := parentalDayString(date)
	if f.usage[accountID] == nil {
		f.usage[accountID] = map[string]*ParentalDailyUsage{}
	}
	cur := f.usage[accountID][key]
	if cur == nil {
		cur = &ParentalDailyUsage{AccountID: accountID, Date: parentalDayKey(date)}
		f.usage[accountID][key] = cur
	}
	cur.UsedSeconds += seconds
	lp := lastPolled
	cur.LastPolledAt = &lp
	return nil
}

func (f *fakeStore) SetParentalBufferStart(_ context.Context, accountID int, date time.Time, t time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	key := parentalDayString(date)
	if f.usage[accountID] == nil {
		f.usage[accountID] = map[string]*ParentalDailyUsage{}
	}
	cur := f.usage[accountID][key]
	if cur == nil {
		cur = &ParentalDailyUsage{AccountID: accountID, Date: parentalDayKey(date)}
		f.usage[accountID][key] = cur
	}
	if cur.BufferStartedAt == nil {
		b := t
		cur.BufferStartedAt = &b
	}
	return nil
}

func (f *fakeStore) UseParentalExtension(_ context.Context, accountID int, date time.Time, t time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	key := parentalDayString(date)
	if f.usage[accountID] == nil {
		f.usage[accountID] = map[string]*ParentalDailyUsage{}
	}
	cur := f.usage[accountID][key]
	if cur == nil {
		cur = &ParentalDailyUsage{AccountID: accountID, Date: parentalDayKey(date)}
		f.usage[accountID][key] = cur
	}
	if cur.ExtendedAt != nil {
		return ErrExtensionUsed
	}
	e := t
	cur.ExtendedAt = &e
	return nil
}

func (f *fakeStore) CreateParentalNotification(_ context.Context, n *ParentalNotification) (int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	cp := *n
	cp.ID = f.nextNotifID
	f.nextNotifID++
	cp.CreatedAt = time.Now()
	cp.RecipientEmailEnc = copyBytes(n.RecipientEmailEnc)
	f.notifications[n.AccountID] = append(f.notifications[n.AccountID], &cp)
	return cp.ID, nil
}

func (f *fakeStore) ListPendingParentalNotifications(_ context.Context, accountID int) ([]ParentalNotification, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []ParentalNotification{}
	for _, n := range f.notifications[accountID] {
		if n.DeliveredAt == nil {
			out = append(out, *n)
		}
	}
	return out, nil
}

func (f *fakeStore) MarkParentalNotificationsDelivered(_ context.Context, accountID int, ids []int, t time.Time) ([]ParentalNotification, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	want := map[int]bool{}
	for _, id := range ids {
		want[id] = true
	}
	out := []ParentalNotification{}
	for _, n := range f.notifications[accountID] {
		if want[n.ID] && n.DeliveredAt == nil {
			d := t
			n.DeliveredAt = &d
			out = append(out, *n)
		}
	}
	return out, nil
}
