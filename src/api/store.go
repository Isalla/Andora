package main

import (
	"context"
	"database/sql"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

// Session is the persisted state of one login session.
type Session struct {
	TokenHash string
	AccountID int
	CreatedAt time.Time
	ExpiresAt time.Time
}

// Realm is a persistent Andora world with the operator-facing
// registration data.
type Realm struct {
	ID              int
	Name            string
	Language        string
	Region          string
	Enabled         bool
	FreshStartUntil *time.Time
	TransferPolicy  string
}

// WorldServer is one technical process executing a Realm.
type WorldServer struct {
	ID             int
	RealmID        int
	Name           string
	Host           string
	Port           int
	Version        string
	Enabled        bool
	MaxPlayers     int
	Credential     string
	Status         string
	CurrentPlayers int
	LastHeartbeat  *time.Time
}

// Handoff is a single-use hand-over token.
type Handoff struct {
	TokenHash string
	AccountID int
	RealmID   int
	CreatedAt time.Time
	ExpiresAt time.Time
	UsedAt    *time.Time
}

// Recovery is a single-use password-reset record.
type Recovery struct {
	TokenHash string
	AccountID int
	CreatedAt time.Time
	ExpiresAt time.Time
	UsedAt    *time.Time
}

// AuthStore is the abstraction over the auth-database operations the
// service performs. The production implementation is sQLStore; tests
// use a fake, so endpoint behaviour is testable without a database.
type AuthStore interface {
	FetchAccount(ctx context.Context, column string, value interface{}) (*Account, error)
	FetchAccountByID(ctx context.Context, id int) (*Account, error)
	RegisterAccount(ctx context.Context, username, passwordHash, emailEncrypted string, emailLookupHash []byte, ttl time.Duration) (int, error)
	GrantAccountPermission(ctx context.Context, accountID int, permission string) error
	ListAccountPermissions(ctx context.Context, accountID int) ([]string, error)
	TouchLogin(ctx context.Context, accountID int) error
	CreateSession(ctx context.Context, accountID int, ttl time.Duration) (string, Session, error)
	ValidateSession(ctx context.Context, rawToken string) (*Session, error)
	RevokeSession(ctx context.Context, sessionID string) error
	CreateHandoff(ctx context.Context, accountID, realmID int, ttl time.Duration) (string, time.Time, error)
	ValidateHandoff(ctx context.Context, rawToken string) (*Handoff, error)
	ListRealms(ctx context.Context) ([]Realm, error)
	FetchWorldServer(ctx context.Context, id int) (*WorldServer, error)
	AuthenticateWorld(ctx context.Context, id int, credential string) (*WorldServer, error)
	RecordHeartbeat(ctx context.Context, id int, version string, currentPlayers int, ok bool) error
	UpdatePassword(ctx context.Context, accountID int, newPasswordHash string) error
	CreateRecoveryToken(ctx context.Context, accountID int, ttl time.Duration) (string, Recovery, error)
	ValidateRecovery(ctx context.Context, rawToken string) (*Recovery, error)
	RecoverPassword(ctx context.Context, accountID int, newPasswordHash string, rawRecoveryToken string) error
	UseHandoff(ctx context.Context, rawToken string) error
}

// sQLStore is the production AuthStore on top of the auth database.
type sQLStore struct {
	db *sql.DB
}

func newSQLStore(db *sql.DB) *sQLStore { return &sQLStore{db: db} }

// openAuthDB connects to the auth database and verifies the
// connection. Fails hard if credentials are wrong or the DB is
// unreachable.
func openAuthDB(ctx context.Context, cfg AuthDBConfig) (*sql.DB, error) {
	dsn := fmt.Sprintf("%s:%s@tcp(%s:%d)/%s?parseTime=true&timeout=5s&readTimeout=5s&writeTimeout=5s",
		cfg.User, cfg.Password, cfg.Host, cfg.Port, cfg.Database)
	db, err := sql.Open("mysql", dsn)
	if err != nil {
		return nil, fmt.Errorf("open auth db: %w", err)
	}
	db.SetMaxOpenConns(10)
	db.SetMaxIdleConns(5)
	db.SetConnMaxLifetime(5 * time.Minute)
	if err := db.PingContext(ctx); err != nil {
		db.Close()
		return nil, fmt.Errorf("connect to auth db (host %s:%d, user %s, db %s): %w",
			cfg.Host, cfg.Port, cfg.User, cfg.Database, err)
	}
	return db, nil
}

// migrationsDir resolves the auth migrations directory: the
// AUTHAPI_MIGRATIONS_DIR override, else <exe>/db/auth/migrations.
func migrationsDir(cfg *Config) (string, error) {
	if cfg.MigrationsDir != "" {
		return cfg.MigrationsDir, nil
	}
	exe, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("locate executable: %w", err)
	}
	return filepath.Join(filepath.Dir(exe), "db", "auth", "migrations"), nil
}

// applyAuthMigrations ensures the schema is at the latest version by
// running any not-yet-applied, sequentially numbered migrations.
// Existing entries in db_version are never modified.
func applyAuthMigrations(ctx context.Context, db *sql.DB, cfg *Config) error {
	if _, err := db.ExecContext(ctx,
		`CREATE TABLE IF NOT EXISTS db_version (
			version INT NOT NULL PRIMARY KEY,
			migration VARCHAR(255) NOT NULL,
			applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
		)`); err != nil {
		return fmt.Errorf("create db_version: %w", err)
	}

	dir, err := migrationsDir(cfg)
	if err != nil {
		return err
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return fmt.Errorf("read migrations dir %s: %w", dir, err)
	}

	applied := make(map[int]bool)
	rows, err := db.QueryContext(ctx, "SELECT version FROM db_version")
	if err != nil {
		return fmt.Errorf("read db_version: %w", err)
	}
	for rows.Next() {
		var v int
		if err := rows.Scan(&v); err != nil {
			rows.Close()
			return fmt.Errorf("scan db_version: %w", err)
		}
		applied[v] = true
	}
	rows.Close()

	names := make([]string, 0, len(entries))
	for _, e := range entries {
		n := e.Name()
		if e.IsDir() || !strings.HasSuffix(n, ".sql") {
			continue
		}
		names = append(names, n)
	}
	sort.Strings(names)

	for _, name := range names {
		base := strings.TrimSuffix(name, ".sql")
		dot := strings.LastIndex(base, "_")
		if dot == -1 {
			return fmt.Errorf("migration %q must contain an underscore separating number and name", name)
		}
		numStr, tag := base[:dot], base[dot+1:]
		if !isDigits(numStr) {
			return fmt.Errorf("migration %q does not start with a numeric prefix", name)
		}
		num, err := atoiPos(numStr)
		if err != nil {
			return fmt.Errorf("parse migration %q: %w", name, err)
		}
		if applied[num] {
			continue
		}
		body, err := os.ReadFile(filepath.Join(dir, name))
		if err != nil {
			return fmt.Errorf("read migration %s: %w", name, err)
		}
		for _, stmt := range splitSQL(string(body)) {
			if _, err := db.ExecContext(ctx, stmt); err != nil {
				return fmt.Errorf("apply migration %d (%s): %w", num, name, err)
			}
		}
		if _, err := db.ExecContext(ctx,
			"INSERT INTO db_version (version, migration) VALUES (?, ?)",
			num, tag); err != nil {
			return fmt.Errorf("record migration %d (%s): %w", num, name, err)
		}
	}
	return nil
}

// isDuplicateKey reports whether a mysql error is a duplicate-key
// (unique constraint) violation. It is used to turn a session or
// permission clash into the semantically correct 409 in the handler.
func isDuplicateKey(err error) bool {
	if err == nil {
		return false
	}
	msg := err.Error()
	return strings.Contains(msg, "1062") || strings.Contains(msg, "Duplicate entry")
}

// splitSQL splits a migration body into the individual SQL statements
// it contains (migration files may carry several, e.g. a table plus
// its index). Lines starting with "--" are comments.
func splitSQL(body string) []string {
	var stmts []string
	cur := make([]string, 0, 8)
	for _, line := range strings.Split(body, "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "--") {
			continue
		}
		cur = append(cur, line)
		if strings.HasSuffix(trimmed, ";") {
			stmt := strings.TrimSpace(strings.Join(cur, " "))
			stmts = append(stmts, strings.TrimSuffix(stmt, ";"))
			cur = make([]string, 0, 8)
		}
	}
	if rest := strings.TrimSpace(strings.Join(cur, " ")); rest != "" {
		stmts = append(stmts, rest)
	}
	return stmts
}

func isDigits(s string) bool {
	if s == "" {
		return false
	}
	for _, r := range s {
		if r < '0' || r > '9' {
			return false
		}
	}
	return true
}

func atoiPos(s string) (int, error) {
	n, err := strconv.Atoi(s)
	return n, err
}

// --- account ---

// FetchAccount returns the minimal account data needed for a login
// check, looked up by username (unique) or by the non-reversible
// email lookup hash (unique, 32 raw bytes).
func (s *sQLStore) FetchAccount(ctx context.Context, column string, value interface{}) (*Account, error) {
	if column != "username" && column != "email_lookup_hash" {
		return nil, fmt.Errorf("unsupported lookup column %q", column)
	}
	row := s.db.QueryRowContext(ctx,
		"SELECT id, username, password_hash, ban_until FROM accounts WHERE "+column+" = ?",
		value)
	acc := &Account{}
	err := row.Scan(&acc.ID, &acc.Username, &acc.PasswordHash, &acc.BanUntil)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("lookup account: %w", err)
	}
	return acc, nil
}

func (s *sQLStore) FetchAccountByID(ctx context.Context, id int) (*Account, error) {
	row := s.db.QueryRowContext(ctx,
		"SELECT id, username, password_hash, ban_until FROM accounts WHERE id = ?", id)
	acc := &Account{}
	err := row.Scan(&acc.ID, &acc.Username, &acc.PasswordHash, &acc.BanUntil)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("lookup account by id: %w", err)
	}
	return acc, nil
}

func (s *sQLStore) RegisterAccount(ctx context.Context, username, passwordHash, emailEncrypted string, emailLookupHash []byte, ttl time.Duration) (int, error) {
	res, err := s.db.ExecContext(ctx,
		`INSERT INTO accounts (username, password_hash, email_encrypted, email_lookup_hash, ban_until)
		 VALUES (?, ?, ?, ?, NOW() + INTERVAL ? SECOND)`,
		username, passwordHash, emailEncrypted, emailLookupHash, int(ttl.Seconds()))
	if err != nil {
		return 0, fmt.Errorf("insert account: %w", err)
	}
	id, err := res.LastInsertId()
	return int(id), err
}

// TouchLogin records the last successful login time.
func (s *sQLStore) TouchLogin(ctx context.Context, accountID int) error {
	_, err := s.db.ExecContext(ctx,
		"UPDATE accounts SET last_login_at = NOW() WHERE id = ?", accountID)
	if err != nil {
		return fmt.Errorf("touch last_login: %w", err)
	}
	return nil
}

// GrantAccountPermission adds a permission to the account (idempotent
// via the unique (account_id, permission) key).
func (s *sQLStore) GrantAccountPermission(ctx context.Context, accountID int, permission string) error {
	if _, err := s.db.ExecContext(ctx,
		`INSERT IGNORE INTO account_permissions (account_id, permission) VALUES (?, ?)`,
		accountID, permission); err != nil {
		return fmt.Errorf("grant permission: %w", err)
	}
	return nil
}

// ListAccountPermissions returns the account's granted permissions.
func (s *sQLStore) ListAccountPermissions(ctx context.Context, accountID int) ([]string, error) {
	rows, err := s.db.QueryContext(ctx,
		"SELECT permission FROM account_permissions WHERE account_id = ? ORDER BY permission",
		accountID)
	if err != nil {
		return nil, fmt.Errorf("list permissions: %w", err)
	}
	defer rows.Close()
	out := []string{}
	for rows.Next() {
		var p string
		if err := rows.Scan(&p); err != nil {
			return nil, fmt.Errorf("scan permission: %w", err)
		}
		out = append(out, p)
	}
	return out, rows.Err()
}

func (s *sQLStore) UpdatePassword(ctx context.Context, accountID int, newPasswordHash string) error {
	res, err := s.db.ExecContext(ctx,
		"UPDATE accounts SET password_hash = ? WHERE id = ?", newPasswordHash, accountID)
	if err != nil {
		return fmt.Errorf("update password: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("update password: %w", err)
	}
	if n == 0 {
		return sql.ErrNoRows
	}
	return nil
}

// --- sessions ---

// CreateSession stores the session by its SHA-256 hash and returns
// the raw opaque token (sent as session_id) plus the persisted row.
// The expiry is persisted (never derived from a fixed time on read).
func (s *sQLStore) CreateSession(ctx context.Context, accountID int, ttl time.Duration) (string, Session, error) {
	now, raw, err := randomToken()
	if err != nil {
		return "", Session{}, err
	}
	exp := now.Add(ttl)
	if _, err := s.db.ExecContext(ctx,
		`INSERT INTO sessions (token_hash, account_id, created_at, expires_at)
		 VALUES (?, ?, ?, ?)`,
		tokenHash(raw), accountID, now, exp); err != nil {
		if isDuplicateKey(err) {
			return "", Session{}, sql.ErrNoRows
		}
		return "", Session{}, fmt.Errorf("insert session: %w", err)
	}
	return raw, Session{TokenHash: tokenHash(raw), AccountID: accountID, CreatedAt: now, ExpiresAt: exp}, nil
}

func (s *sQLStore) ValidateSession(ctx context.Context, rawToken string) (*Session, error) {
	row := s.db.QueryRowContext(ctx,
		`SELECT token_hash, account_id, created_at, expires_at
		 FROM sessions WHERE token_hash = ?`,
		tokenHash(rawToken))
	sess := &Session{}
	err := row.Scan(&sess.TokenHash, &sess.AccountID, &sess.CreatedAt, &sess.ExpiresAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("lookup session: %w", err)
	}
	if sess.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	return sess, nil
}

func (s *sQLStore) RevokeSession(ctx context.Context, sessionID string) error {
	if !isHexToken(sessionID) {
		return fmt.Errorf("bad session token")
	}
	if _, err := s.db.ExecContext(ctx,
		"DELETE FROM sessions WHERE token_hash = ?", tokenHash(sessionID)); err != nil {
		return fmt.Errorf("revoke session: %w", err)
	}
	return nil
}

// --- realms / world servers / handoffs ---

func (s *sQLStore) ListRealms(ctx context.Context) ([]Realm, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT id, name, language, region, enabled, fresh_start_until, transfer_policy
		 FROM realms ORDER BY id`)
	if err != nil {
		return nil, fmt.Errorf("list realms: %w", err)
	}
	defer rows.Close()
	out := []Realm{}
	for rows.Next() {
		var r Realm
		if err := rows.Scan(&r.ID, &r.Name, &r.Language, &r.Region, &r.Enabled, &r.FreshStartUntil, &r.TransferPolicy); err != nil {
			return nil, fmt.Errorf("scan realm: %w", err)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

func (s *sQLStore) FetchWorldServer(ctx context.Context, id int) (*WorldServer, error) {
	row := s.db.QueryRowContext(ctx,
		`SELECT id, realm_id, name, host, port, version, enabled, max_players, credential, status, current_players, last_heartbeat
		 FROM world_servers WHERE id = ?`, id)
	ws := &WorldServer{}
	err := row.Scan(&ws.ID, &ws.RealmID, &ws.Name, &ws.Host, &ws.Port, &ws.Version,
		&ws.Enabled, &ws.MaxPlayers, &ws.Credential, &ws.Status, &ws.CurrentPlayers, &ws.LastHeartbeat)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("lookup world server: %w", err)
	}
	return ws, nil
}

// AuthenticateWorld checks the presented world-server credential
// against the operator-registered row (constant-time compare).
func (s *sQLStore) AuthenticateWorld(ctx context.Context, id int, credential string) (*WorldServer, error) {
	ws, err := s.FetchWorldServer(ctx, id)
	if err != nil {
		return nil, err
	}
	if ws == nil || ws.Credential == "" || !ws.Enabled || !hmacEqual(ws.Credential, credential) {
		return nil, sql.ErrNoRows
	}
	return ws, nil
}

func (s *sQLStore) RecordHeartbeat(ctx context.Context, id int, version string, currentPlayers int, ok bool) error {
	status := "online"
	if !ok {
		status = "offline"
	}
	res, err := s.db.ExecContext(ctx,
		`UPDATE world_servers SET last_heartbeat = NOW(), version = ?,
			 current_players = ?, status = ? WHERE id = ?`,
		version, currentPlayers, status, id)
	if err != nil {
		return fmt.Errorf("record heartbeat: %w", err)
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return sql.ErrNoRows
	}
	return nil
}

func (s *sQLStore) CreateHandoff(ctx context.Context, accountID, realmID int, ttl time.Duration) (string, time.Time, error) {
	now, raw, err := randomToken()
	if err != nil {
		return "", time.Time{}, err
	}
	exp := now.Add(ttl)
	if _, err := s.db.ExecContext(ctx,
		`INSERT INTO handoffs (token_hash, account_id, realm_id, created_at, expires_at, used_at)
		 VALUES (?, ?, ?, ?, ?, NULL)`,
		tokenHash(raw), accountID, realmID, now, exp); err != nil {
		return "", time.Time{}, fmt.Errorf("insert handoff: %w", err)
	}
	return raw, exp, nil
}

func (s *sQLStore) ValidateHandoff(ctx context.Context, rawToken string) (*Handoff, error) {
	row := s.db.QueryRowContext(ctx,
		`SELECT token_hash, account_id, realm_id, created_at, expires_at, used_at
		 FROM handoffs WHERE token_hash = ? AND used_at IS NULL`,
		tokenHash(rawToken))
	h := &Handoff{}
	err := row.Scan(&h.TokenHash, &h.AccountID, &h.RealmID, &h.CreatedAt, &h.ExpiresAt, &h.UsedAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("lookup handoff: %w", err)
	}
	if h.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	return h, nil
}

// useHandoff marks the handoff as used; a token cannot be validated a
// second time.
func (s *sQLStore) UseHandoff(ctx context.Context, rawToken string) error {
	res, err := s.db.ExecContext(ctx,
		`UPDATE handoffs SET used_at = NOW()
		 WHERE token_hash = ? AND used_at IS NULL`, tokenHash(rawToken))
	if err != nil {
		return fmt.Errorf("use handoff: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("use handoff: %w", err)
	}
	return errWrapNoRows(n)
}

// --- recovery ---

func (s *sQLStore) CreateRecoveryToken(ctx context.Context, accountID int, ttl time.Duration) (string, Recovery, error) {
	now, raw, err := randomToken()
	if err != nil {
		return "", Recovery{}, err
	}
	exp := now.Add(ttl)
	if _, err := s.db.ExecContext(ctx,
		`INSERT INTO recoveries (token_hash, account_id, created_at, expires_at, used_at)
		 VALUES (?, ?, ?, ?, NULL)`,
		tokenHash(raw), accountID, now, exp); err != nil {
		return "", Recovery{}, fmt.Errorf("insert recovery: %w", err)
	}
	return raw, Recovery{TokenHash: tokenHash(raw), AccountID: accountID, CreatedAt: now, ExpiresAt: exp}, nil
}

func (s *sQLStore) ValidateRecovery(ctx context.Context, rawToken string) (*Recovery, error) {
	row := s.db.QueryRowContext(ctx,
		`SELECT token_hash, account_id, created_at, expires_at, used_at
		 FROM recoveries WHERE token_hash = ? AND used_at IS NULL`,
		tokenHash(rawToken))
	r := &Recovery{}
	if err := row.Scan(&r.TokenHash, &r.AccountID, &r.CreatedAt, &r.ExpiresAt, &r.UsedAt); err == sql.ErrNoRows {
		return nil, nil
	} else if err != nil {
		return nil, fmt.Errorf("lookup recovery: %w", err)
	}
	if r.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	return r, nil
}

// RecoverPassword sets the new password, clears the ban and consumes
// the recovery token in one transaction.
func (s *sQLStore) RecoverPassword(ctx context.Context, accountID int, newPasswordHash string, rawRecoveryToken string) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin recovery: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		`UPDATE accounts SET password_hash = ?, ban_until = NULL WHERE id = ?`,
		newPasswordHash, accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("recover password: %w", err)
	}
	res, err := tx.ExecContext(ctx,
		`UPDATE recoveries SET used_at = NOW() WHERE token_hash = ? AND used_at IS NULL`,
		tokenHash(rawRecoveryToken))
	if err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("recover password: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("recover password: %w", err)
	}
	if n == 0 {
		_ = tx.Rollback()
		return sql.ErrNoRows
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit recovery: %w", err)
	}
	return nil
}

// --- helpers ---

func errWrapNoRows(affected int64) error {
	if affected == 0 {
		return sql.ErrNoRows
	}
	return nil
}

// tokenHash is the SHA-256 hex of a token; tokens are stored hashed
// so a DB dump never exposes usable credential material.
func tokenHash(raw string) string { return sha256Hex([]byte(raw)) }

func isHexToken(s string) bool {
	if len(s) != 64 {
		return false
	}
	_, err := hex.DecodeString(s)
	return err == nil
}
