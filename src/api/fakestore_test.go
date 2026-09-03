package main

import (
	"context"
	"database/sql"
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
}

func newFakeStore() *fakeStore {
	return &fakeStore{
		accounts:   map[int]*Account{},
		nextID:     1,
		byName:     map[string]int{},
		byEmail:    map[string]int{},
		sessions:   map[string]Session{},
		handoffs:   map[string]*Handoff{},
		recoveries: map[string]*Recovery{},
		worlds:     map[int]*WorldServer{},
		perms:      map[int]map[string]bool{},
		lastLogin:  map[int]*time.Time{},
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
	if sess.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	s := sess
	return &s, nil
}

func (f *fakeStore) RevokeSession(_ context.Context, sessionID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	delete(f.sessions, tokenHash(sessionID))
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
	acc.PasswordHash = newPasswordHash
	acc.BanUntil = nil
	now := time.Now()
	rec.UsedAt = &now
	return nil
}
