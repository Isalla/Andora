package main

import (
	"context"
	"database/sql"
	"encoding/hex"
	"errors"
	"fmt"
	"log"
	"net"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	mysqlerr "github.com/go-sql-driver/mysql"
)

// Session is the persisted state of one login session.
type Session struct {
	TokenHash string
	AccountID int
	CreatedAt time.Time
	ExpiresAt time.Time
	// RevokedAt marks an EXPLICIT revocation (session.revoke, password
	// change, password reset, 2FA reset). nil means "never explicitly
	// revoked" — an expired session therefore keeps revoked_at = NULL and
	// stays distinguishable from a revoked one (docs/Security.md AUTH-02b).
	RevokedAt *time.Time
}

// SessionStatus is the internal four-way session state. The distinction
// between Expired and Revoked is what lets a realm keep a connection on
// normal TTL expiry while closing it on explicit revocation.
//
// The EXTERNAL /session/validate response stays backward compatible: only
// Valid yields valid:true. Only the batch endpoint exposes the distinction.
type SessionStatus string

const (
	// SessionValid: row present, revoked_at IS NULL, expires_at in the future.
	SessionValid SessionStatus = "valid"
	// SessionExpired: row present, revoked_at IS NULL, expires_at in the past.
	// Must NOT disconnect an already authenticated realm connection
	// (docs/Security.md AUTH-02a).
	SessionExpired SessionStatus = "expired"
	// SessionRevoked: row present with revoked_at set. Explicit revocation.
	SessionRevoked SessionStatus = "revoked"
	// SessionMissing: no row at all — unknown token, or a revocation that
	// predates migration 014 and therefore deleted the row.
	SessionMissing SessionStatus = "missing"
)

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

// TrustedDevice is a confirmed device bound to an account. The raw
// device token itself is never stored; only its SHA-256 hash is.
// LastUsedAt is the last successful login this device presented; it
// carries the 30-day inactivity expiry (see trustedDeviceInactivity).
type TrustedDevice struct {
	ID          int
	AccountID   int
	TokenHash   string
	Label       string
	ConfirmedAt time.Time
	CreatedAt   time.Time
	LastUsedAt  time.Time
}

// RecoveryCode is a single-use 2FA recovery code (hashed in the DB).
type RecoveryCode struct {
	TokenHash string
	AccountID int
	CreatedAt time.Time
	UsedAt    *time.Time
}

// SecurityEvent is one record in the security event log.
type SecurityEvent struct {
	ID        int
	EventType string
	AccountID *int
	CreatedAt time.Time
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
	// SessionStatusOf / BatchSessionStatus expose the four-way status
	// (valid/expired/revoked/missing) needed by a realm revocation poller
	// (docs/Security.md AUTH-02b). The single /session/validate endpoint
	// keeps its boolean contract and does not use them.
	SessionStatusOf(ctx context.Context, rawToken string) (SessionStatus, int, error)
	BatchSessionStatus(ctx context.Context, tokens []string) ([]SessionStatus, []int, error)
	// SessionInventory and SessionTableSize back the P-33 session
	// monitoring. Both are strictly read-only and return aggregates or
	// estimates only: no token, session id, account id or username ever
	// leaves the store. Neither deletes nor revokes anything.
	SessionInventory(ctx context.Context) (SessionInventory, error)
	SessionTableSize(ctx context.Context) (SessionTableSize, error)
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

	// --- TOTP-2FA / trusted devices / recovery codes / security events ---

	SetTwoFactorEnabled(ctx context.Context, accountID int, enabled bool) error
	SaveTwoFactorSecret(ctx context.Context, accountID int, encryptedSecret []byte) error
	ClearTwoFactorSecret(ctx context.Context, accountID int) error
	SetLastTOTP(ctx context.Context, accountID int, counter int, at time.Time) error

	// SetupTwoFactor, EnableTwoFactor, DisableTwoFactor and ResetTwoFactor
	// are the four business 2FA operations. Each is ONE transaction whose
	// security state change and its security_events row commit together; a
	// failing event write rolls the whole operation back (P-34). The
	// single-column helpers above stay generic and never invent an event of
	// their own; RevokeAllSessions and RevokeAllTrustedDevices likewise stay
	// event-less (docs/Auth_API_Architektur.md §18).
	//
	// SetupTwoFactor reports a conflict via ErrTwoFactorAlreadySetUp.
	// EnableTwoFactor and DisableTwoFactor return changed=false for a no-op
	// (already enabled / already disabled) and write no event.
	SetupTwoFactor(ctx context.Context, accountID int, encryptedSecret []byte, codes []string) error
	EnableTwoFactor(ctx context.Context, accountID int) (changed bool, err error)
	DisableTwoFactor(ctx context.Context, accountID int) (changed bool, err error)
	// ResetTwoFactor rotates the whole 2FA state in one transaction. The
	// CAS on expectedSecret keeps two requests that read the SAME stored
	// secret from both committing: only the first gets changed=true, the
	// loser gets changed=false, err == nil and no state, no event and no
	// recovery codes. A later reset that read the already rotated state is
	// a new valid operation and commits normally (last commit wins).
	ResetTwoFactor(ctx context.Context, accountID int, expectedSecret, newSecret []byte, codes []string) (changed bool, err error)

	// ChangePasswordRevokeAll swaps the password hash, revokes ALL
	// sessions and ALL trusted-device tokens in one transaction and
	// records the security event.
	ChangePasswordRevokeAll(ctx context.Context, accountID int, newPasswordHash string) error

	CountTrustedDevices(ctx context.Context, accountID int) (int, error)
	AddTrustedDevice(ctx context.Context, accountID int, rawToken, label string) (TrustedDevice, error)
	HasTrustedDevice(ctx context.Context, accountID int, rawToken string) (bool, error)
	TouchTrustedDevice(ctx context.Context, accountID int, rawToken string) error
	RevokeTrustedDevice(ctx context.Context, accountID int, rawToken string) error
	ListTrustedDevices(ctx context.Context, accountID int) ([]TrustedDevice, error)
	RevokeAllTrustedDevices(ctx context.Context, accountID int) error

	SaveRecoveryCodes(ctx context.Context, accountID int, codes []string) error
	UseRecoveryCode(ctx context.Context, accountID int, rawCode string) error
	ClearRecoveryCodes(ctx context.Context, accountID int) error
	CountRecoveryCodes(ctx context.Context, accountID int) (int, error)

	// RevokeAllSessions drops every live session for the account (used
	// by the 2FA reset path).
	RevokeAllSessions(ctx context.Context, accountID int) error

	RecordSecurityEvent(ctx context.Context, eventType string, accountID *int) error
	ListSecurityEvents(ctx context.Context, accountID int) ([]SecurityEvent, error)

	// --- parental control (migration 013) ---
	//
	// The account row only carries the parental_control_enabled flag;
	// the full rule set lives in the parental_* tables. Day limits use
	// minutes with the convention 0 = unlimited (weekly rules, special
	// periods and exception overrides alike).

	FetchParentalControls(ctx context.Context, accountID int) (*ParentalControls, error)
	CreateParentalControl(ctx context.Context, ctl *ParentalControls) error
	UpdateParentalControls(ctx context.Context, ctl *ParentalControls) error
	SetParentPinHash(ctx context.Context, accountID int, hash string) error
	SetParentEmailEnc(ctx context.Context, accountID int, enc []byte) error
	SetParentalEnabled(ctx context.Context, accountID int, enabled bool) error
	DeleteParentalControls(ctx context.Context, accountID int) error
	ListParentalPeriods(ctx context.Context, accountID int) ([]ParentalPeriod, error)
	AddParentalPeriod(ctx context.Context, p *ParentalPeriod) (int, error)
	DeleteParentalPeriod(ctx context.Context, accountID, id int) error
	FetchParentalException(ctx context.Context, accountID int, date time.Time) (*ParentalException, error)
	SaveParentalException(ctx context.Context, e *ParentalException) (int, error)
	DeleteParentalException(ctx context.Context, accountID int, date time.Time) error
	GetActiveParentalPeriod(ctx context.Context, accountID int, date time.Time) (*ParentalPeriod, error)
	FetchParentalUsage(ctx context.Context, accountID int, date time.Time) (ParentalDailyUsage, error)
	AddParentalUsageSeconds(ctx context.Context, accountID int, date time.Time, seconds int, lastPolled time.Time, dayLimit int) error
	SetParentalBufferStart(ctx context.Context, accountID int, date time.Time, t time.Time) error
	UseParentalExtension(ctx context.Context, accountID int, date time.Time, t time.Time) error
	CreateParentalNotification(ctx context.Context, n *ParentalNotification) (int, error)
	ListPendingParentalNotifications(ctx context.Context, accountID int) ([]ParentalNotification, error)
	MarkParentalNotificationsDelivered(ctx context.Context, accountID int, ids []int, t time.Time) ([]ParentalNotification, error)
}

// ErrMaxDevices reports that the 3-device limit is reached; it is the
// service-level signal for the 409 max_devices_reached response.
var ErrMaxDevices = fmt.Errorf("max trusted devices reached")

// ErrTwoFactorAlreadySetUp reports that the transactional setup gate lost a
// race: another setup committed first. The loser wrote no secret, no
// recovery codes and no event; the handler maps it to the existing 409.
var ErrTwoFactorAlreadySetUp = fmt.Errorf("two-factor already set up")

// maxTrustedDevices is the service-level limit of confirmed devices per
// account. It is enforced in AddTrustedDevice (not as a DB constraint)
// because revocation must happen BEFORE a new confirmation when the
// limit is reached. The limit only counts ACTIVE devices: an expired
// device frees its slot before the next confirmation.
const maxTrustedDevices = 3

// trustedDeviceInactivity is the 30-day window after the last
// successful login with a confirmed device. The expiry lives in the
// trusted_devices table itself (last_used_at) and never in accounts.
const trustedDeviceInactivity = 30 * 24 * time.Hour

// trustedDeviceActive reports whether a confirmed device is still
// valid, i.e. its last usage is younger than trustedDeviceInactivity.
// Exactly 30 days is still active; one nanosecond beyond is expired.
func trustedDeviceActive(lastUsed, now time.Time) bool {
	return !now.After(lastUsed.Add(trustedDeviceInactivity))
}

// Security event types (closed set; migration 011). Account-scoped
// events only; there is no mail engine behind them.
const (
	eventDeviceConfirmed   = "device_confirmed"
	eventDeviceRevoked     = "device_revoked"
	eventTwoFactorEnabled  = "two_factor_enabled"
	eventTwoFactorDisabled = "two_factor_disabled"
	eventTwoFactorReset    = "two_factor_reset"
	eventPasswordChanged   = "password_changed"
	eventPasswordReset     = "password_reset"
	eventRecoveryCodeUsed  = "recovery_code_used"
)

// hashRecoveryCode is the SHA-256 of the normalized recovery code
// (upper case, separators + whitespace stripped). It is the value
// persisted in recovery_codes.token_hash; cosmetic variants of the same
// code hash identically.
func hashRecoveryCode(rawCode string) string {
	return sha256Hex([]byte(normalizeRecoveryCode(rawCode)))
}

// sQLStore is the production AuthStore on top of the auth database.
type sQLStore struct {
	db *sql.DB
	// batchTiming measures the productive batch session-status lookup
	// (P-33). Zero value is ready to use.
	batchTiming batchLookupTiming
}

func newSQLStore(db *sql.DB) *sQLStore { return &sQLStore{db: db} }

// mysqlDSN builds the DSN for the go-sql-driver/mysql connection. An
// IPv6 host is bracketed via net.JoinHostPort.
func mysqlDSN(cfg AuthDBConfig) string {
	return fmt.Sprintf("%s:%s@tcp(%s)/%s?parseTime=true&timeout=5s&readTimeout=5s&writeTimeout=5s",
		cfg.User, cfg.Password, net.JoinHostPort(cfg.Host, strconv.Itoa(cfg.Port)), cfg.Database)
}

// openAuthDB connects to the auth database and verifies the
// connection. Fails hard if credentials are wrong or the DB is
// unreachable.
func openAuthDB(ctx context.Context, cfg AuthDBConfig) (*sql.DB, error) {
	db, err := sql.Open("mysql", mysqlDSN(cfg))
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
		// Split at the FIRST underscore: names are NNN_rest_of_name.sql
		// and the rest may itself contain underscores
		// (e.g. 004_world_servers.sql -> 004 / world_servers).
		dot := strings.Index(base, "_")
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

// accountColumns is the select list every account fetch shares: the
// login fields plus the TOTP-2FA state (migration 008).
const accountColumns = `id, username, password_hash, ban_until,
	two_factor_enabled, two_factor_secret, last_totp_counter, last_totp_at,
	parental_control_enabled`

// scanAccount maps one row into an Account (nil on ErrNoRows).
func scanAccount(row *sql.Row) (*Account, error) {
	acc := &Account{}
	err := row.Scan(&acc.ID, &acc.Username, &acc.PasswordHash, &acc.BanUntil,
		&acc.TwoFactorEnabled, &acc.TwoFactorSecret, &acc.LastTOTPCounter, &acc.LastTOTPAt,
		&acc.ParentalEnabled)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return acc, nil
}

// FetchAccount returns the minimal account data needed for a login
// check, looked up by username (unique) or by the non-reversible
// email lookup hash (unique, 32 raw bytes).
func (s *sQLStore) FetchAccount(ctx context.Context, column string, value interface{}) (*Account, error) {
	if column != "username" && column != "email_lookup_hash" {
		return nil, fmt.Errorf("unsupported lookup column %q", column)
	}
	row := s.db.QueryRowContext(ctx,
		"SELECT "+accountColumns+" FROM accounts WHERE "+column+" = ?",
		value)
	acc, err := scanAccount(row)
	if err != nil {
		return nil, fmt.Errorf("lookup account: %w", err)
	}
	return acc, nil
}

func (s *sQLStore) FetchAccountByID(ctx context.Context, id int) (*Account, error) {
	row := s.db.QueryRowContext(ctx,
		"SELECT "+accountColumns+" FROM accounts WHERE id = ?", id)
	acc, err := scanAccount(row)
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
		`SELECT token_hash, account_id, created_at, expires_at, revoked_at
		 FROM sessions WHERE token_hash = ?`,
		tokenHash(rawToken))
	sess := &Session{}
	var revoked sql.NullTime
	err := row.Scan(&sess.TokenHash, &sess.AccountID, &sess.CreatedAt, &sess.ExpiresAt, &revoked)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("lookup session: %w", err)
	}
	if revoked.Valid {
		sess.RevokedAt = &revoked.Time
	}
	if sess.RevokedAt != nil || sess.ExpiresAt.Before(time.Now()) {
		return nil, nil
	}
	return sess, nil
}

// sessionStatusOf classifies a fetched row. A revoked row is checked BEFORE
// expiry: an explicitly revoked session is reported as revoked even if its
// expires_at has also passed, because the revocation is the authoritative,
// intentional signal and the realm must not silently downgrade it to expiry.
func sessionStatusOf(sess *Session, now time.Time) SessionStatus {
	if sess.RevokedAt != nil {
		return SessionRevoked
	}
	if sess.ExpiresAt.Before(now) {
		return SessionExpired
	}
	return SessionValid
}

// SessionStatusOf returns the four-way status of one session WITHOUT logging
// the token. Used by the batch endpoint; the single /session/validate
// endpoint keeps its backward-compatible boolean behaviour.
func (s *sQLStore) SessionStatusOf(ctx context.Context, rawToken string) (SessionStatus, int, error) {
	row := s.db.QueryRowContext(ctx,
		`SELECT account_id, expires_at, revoked_at FROM sessions WHERE token_hash = ?`,
		tokenHash(rawToken))
	var accountID int
	var expiresAt time.Time
	var revoked sql.NullTime
	switch err := row.Scan(&accountID, &expiresAt, &revoked); {
	case err == sql.ErrNoRows:
		return SessionMissing, 0, nil
	case err != nil:
		return SessionMissing, 0, fmt.Errorf("lookup session: %w", err)
	}
	sess := &Session{AccountID: accountID, ExpiresAt: expiresAt}
	if revoked.Valid {
		sess.RevokedAt = &revoked.Time
	}
	return sessionStatusOf(sess, time.Now()), accountID, nil
}

// BatchSessionStatus resolves many sessions in ONE query (single IN clause,
// no N+1). Input order is preserved: result[i] belongs to tokens[i].
//
// Tokens are used exclusively for hashing and the lookup; nothing is logged
// and no raw token appears in the response. accountIDs[i] is 0 for a missing
// session and is otherwise the authoritative owner, so a realm can verify
// that a result really belongs to the connection it asked about.
func (s *sQLStore) BatchSessionStatus(ctx context.Context, tokens []string) (statuses []SessionStatus, accountIDs []int, err error) {
	statuses = make([]SessionStatus, len(tokens))
	accountIDs = make([]int, len(tokens))
	for i := range tokens {
		statuses[i] = SessionMissing
	}
	if len(tokens) == 0 {
		return statuses, accountIDs, nil
	}
	// P-33: measure the productive DB lookup including row processing.
	// No additional query is issued; only a fully successful lookup is
	// recorded, so an error keeps the previous measurement intact.
	lookupStart := time.Now()
	// One placeholders string, one query, len(tokens) bound parameters.
	ph := strings.TrimSuffix(strings.Repeat("?,", len(tokens)), ",")
	args := make([]any, 0, len(tokens))
	index := make(map[string]int, len(tokens))
	for i, raw := range tokens {
		h := tokenHash(raw)
		index[h] = i
		args = append(args, h)
	}
	rows, err := s.db.QueryContext(ctx,
		`SELECT token_hash, account_id, expires_at, revoked_at
		 FROM sessions WHERE token_hash IN (`+ph+`)`, args...)
	if err != nil {
		return nil, nil, fmt.Errorf("batch session lookup: %w", err)
	}
	defer rows.Close()
	now := time.Now()
	for rows.Next() {
		var h string
		var accountID int
		var expiresAt time.Time
		var revoked sql.NullTime
		if err := rows.Scan(&h, &accountID, &expiresAt, &revoked); err != nil {
			return nil, nil, fmt.Errorf("scan batch session: %w", err)
		}
		i, ok := index[h]
		if !ok {
			// Unreachable for correct data; ignored deliberately rather than
			// aborting the whole batch.
			continue
		}
		sess := &Session{AccountID: accountID, ExpiresAt: expiresAt}
		if revoked.Valid {
			sess.RevokedAt = &revoked.Time
		}
		statuses[i] = sessionStatusOf(sess, now)
		accountIDs[i] = accountID
	}
	if err := rows.Err(); err != nil {
		return nil, nil, fmt.Errorf("batch session rows: %w", err)
	}
	s.batchTiming.record(time.Since(lookupStart), time.Now())
	return statuses, accountIDs, nil
}

// RevokeSession marks EXACTLY this one session as explicitly revoked. The row
// is kept so that the revocation stays distinguishable from a normal expiry
// (migration 014, docs/Security.md AUTH-02b). Idempotent: re-revoking keeps
// the original revoked_at.
func (s *sQLStore) RevokeSession(ctx context.Context, sessionID string) error {
	if !isHexToken(sessionID) {
		return fmt.Errorf("bad session token")
	}
	if _, err := s.db.ExecContext(ctx,
		`UPDATE sessions SET revoked_at = NOW()
		 WHERE token_hash = ? AND revoked_at IS NULL`, tokenHash(sessionID)); err != nil {
		return fmt.Errorf("revoke session: %w", err)
	}
	return nil
}

// --- session inventory (P-33 monitoring, read-only) ---

// SessionInventory is the aggregated session stock of ONE collection.
// It carries counts and one age in days only; it is deliberately free of
// any identifying data. PartitionOK reports whether the three status
// classes add up to the total and is meant to be carried along as a
// self-check with every collection.
type SessionInventory struct {
	Total             int64
	Active            int64
	ExpiredNotRevoked int64
	Revoked           int64
	PartitionOK       bool
	// OldestRowAgeDays is nil when the table is empty: an unknown age is
	// reported as unknown, never as 0.
	OldestRowAgeDays *int64
	// QueryMS is the duration of the inventory query itself. It is only
	// set on success; a failed query yields the zero value.
	QueryMS float64
}

// SessionTableSize carries the InnoDB ESTIMATES for the sessions table.
// A nil field means "unknown" (metadata row absent, not readable).
// These are estimates, not exact values.
type SessionTableSize struct {
	TableRows  *int64
	DataBytes  *int64
	IndexBytes *int64
}

// errSessionSizeNoAccess marks a metadata query that was rejected
// because of missing privileges. It is a classification, never a raw
// driver message, so it can be reported without leaking details.
var errSessionSizeNoAccess = errors.New("session table size: metadata access denied")

// SessionInventory counts the session rows in ONE aggregate round with a
// single DB-side NOW(), so all counters of one evaluation share one time
// base (docs/Security.md P-33). The classification follows sessionStatusOf:
// revoked is checked BEFORE expiry, which makes the three classes disjoint
// and their sum the total.
//
// created_at and expires_at are written from the Go clock on insert,
// while NOW() is the DB clock; a clock difference can therefore shift a
// row at the expiry border. The existing time sources are NOT changed.
func (s *sQLStore) SessionInventory(ctx context.Context) (SessionInventory, error) {
	start := time.Now()
	var inv SessionInventory
	var oldest sql.NullInt64
	err := s.db.QueryRowContext(ctx,
		`SELECT
		   COUNT(*)                                                                              AS total,
		   COALESCE(SUM(CASE WHEN revoked_at IS NULL AND expires_at >= NOW() THEN 1 ELSE 0 END), 0) AS active,
		   COALESCE(SUM(CASE WHEN revoked_at IS NULL AND expires_at <  NOW() THEN 1 ELSE 0 END), 0) AS expired_not_revoked,
		   COALESCE(SUM(CASE WHEN revoked_at IS NOT NULL THEN 1 ELSE 0 END), 0)                  AS revoked,
		   CASE WHEN COUNT(*) = 0 THEN NULL
		        ELSE TIMESTAMPDIFF(DAY, MIN(created_at), NOW()) END                              AS oldest_row_age_days
		 FROM sessions`).
		Scan(&inv.Total, &inv.Active, &inv.ExpiredNotRevoked, &inv.Revoked, &oldest)
	if err != nil {
		// No counters, no duration: a failed query is not a measurement.
		return SessionInventory{}, fmt.Errorf("session inventory: %w", err)
	}
	if oldest.Valid {
		d := oldest.Int64
		inv.OldestRowAgeDays = &d
	}
	inv.PartitionOK = inv.Total == inv.Active+inv.ExpiredNotRevoked+inv.Revoked
	inv.QueryMS = float64(time.Since(start).Microseconds()) / 1000
	return inv, nil
}

// SessionTableSize reads the InnoDB estimates for the sessions table of
// the CURRENT database. information_schema.TABLES lists only objects the
// user may see, so a missing row means "unknown" and NOT "denied"; a
// genuinely rejected query is classified as errSessionSizeNoAccess.
// The estimates are approximate and are reported as such.
func (s *sQLStore) SessionTableSize(ctx context.Context) (SessionTableSize, error) {
	var rows, data, index sql.NullInt64
	err := s.db.QueryRowContext(ctx,
		`SELECT TABLE_ROWS, DATA_LENGTH, INDEX_LENGTH
		 FROM information_schema.TABLES
		 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sessions'`).
		Scan(&rows, &data, &index)
	switch {
	case errors.Is(err, sql.ErrNoRows):
		// No visible row: unknown, not an error and not zero.
		return SessionTableSize{}, nil
	case err != nil:
		if isMetadataAccessDenied(err) {
			return SessionTableSize{}, errSessionSizeNoAccess
		}
		return SessionTableSize{}, fmt.Errorf("session table size: %w", err)
	}
	out := SessionTableSize{}
	if rows.Valid {
		v := rows.Int64
		out.TableRows = &v
	}
	if data.Valid {
		v := data.Int64
		out.DataBytes = &v
	}
	if index.Valid {
		v := index.Int64
		out.IndexBytes = &v
	}
	return out, nil
}

// isMetadataAccessDenied reports whether err is a MariaDB/MySQL
// privilege rejection. The message text is deliberately not inspected.
func isMetadataAccessDenied(err error) bool {
	var me *mysqlerr.MySQLError
	if !errors.As(err, &me) {
		return false
	}
	switch me.Number {
	case 1044, // ER_DBACCESS_DENIED_ERROR
		1045, // ER_ACCESS_DENIED_ERROR
		1142: // ER_TABLEACCESS_DENIED_ERROR
		return true
	}
	return false
}

// batchLookupTiming holds the measured durations of the PRODUCTIVE batch
// session-status lookup. Deliberately NOT a series: only the last
// successful measurement, the process-wide maximum since start and the
// wall-clock time of that measurement. Concurrent lookups are
// synchronised; a failed lookup records nothing, so a previous success is
// never presented as a new measurement.
type batchLookupTiming struct {
	mu         sync.Mutex
	last       time.Duration
	max        time.Duration
	measuredAt time.Time
	measured   bool
}

// record stores one fully successful lookup measurement.
func (t *batchLookupTiming) record(d time.Duration, at time.Time) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.last = d
	if d > t.max {
		t.max = d
	}
	t.measuredAt = at
	t.measured = true
}

// snapshot returns the current measurement state; ok is false until the
// first successful lookup.
func (t *batchLookupTiming) snapshot() (last, max time.Duration, measuredAt time.Time, ok bool) {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.last, t.max, t.measuredAt, t.measured
}

// batchLookupTimer is implemented by stores that measure the productive
// batch session-status lookup. It is read by the /status collector, not
// by the lookup path itself.
type batchLookupTimer interface {
	BatchLookupTiming() (last, max time.Duration, measuredAt time.Time, ok bool)
}

// BatchLookupTiming returns the last successful lookup duration, the
// process-wide maximum since start and when it was measured. It is NOT
// the HTTP latency of the batch endpoint and contains no percentiles.
func (s *sQLStore) BatchLookupTiming() (time.Duration, time.Duration, time.Time, bool) {
	return s.batchTiming.snapshot()
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

// RecoverPassword sets the new password, clears the ban, consumes the
// recovery token, revokes every session and every trusted device, and records
// the security event — ALL of it in one transaction (AUTH-02b).
//
// The password change and the session revocation must not be separable: a
// partial commit would leave a password the previous owner no longer knows
// next to still-valid logins. Every error path rolls back completely, so the
// old password stays, sessions and trusted devices stay untouched, and the
// recovery token remains reusable.
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
	// Same full security effect as ChangePasswordRevokeAll: sessions are
	// MARKED (not deleted, migration 014) so an explicit revocation stays
	// distinguishable from a normal expiry.
	if _, err := tx.ExecContext(ctx,
		`UPDATE sessions SET revoked_at = NOW()
		 WHERE account_id = ? AND revoked_at IS NULL`, accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("recover password: revoke sessions: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		"DELETE FROM trusted_devices WHERE account_id = ?", accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("recover password: revoke trusted devices: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		"INSERT INTO security_events (event_type, account_id, created_at) VALUES (?, ?, NOW())",
		eventPasswordReset, accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("record recovery event: %w", err)
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

// --- TOTP-2FA / trusted devices / recovery codes / security events ---

func (s *sQLStore) SetTwoFactorEnabled(ctx context.Context, accountID int, enabled bool) error {
	b := 0
	if enabled {
		b = 1
	}
	if _, err := s.db.ExecContext(ctx,
		"UPDATE accounts SET two_factor_enabled = ? WHERE id = ?", b, accountID); err != nil {
		return fmt.Errorf("set two_factor_enabled: %w", err)
	}
	return nil
}

func (s *sQLStore) SaveTwoFactorSecret(ctx context.Context, accountID int, encryptedSecret []byte) error {
	if _, err := s.db.ExecContext(ctx,
		"UPDATE accounts SET two_factor_secret = ?, last_totp_counter = NULL, last_totp_at = NULL WHERE id = ?",
		encryptedSecret, accountID); err != nil {
		return fmt.Errorf("save two_factor_secret: %w", err)
	}
	return nil
}

func (s *sQLStore) ClearTwoFactorSecret(ctx context.Context, accountID int) error {
	if _, err := s.db.ExecContext(ctx,
		"UPDATE accounts SET two_factor_secret = NULL, last_totp_counter = NULL, last_totp_at = NULL WHERE id = ?",
		accountID); err != nil {
		return fmt.Errorf("clear two_factor_secret: %w", err)
	}
	return nil
}

func (s *sQLStore) SetLastTOTP(ctx context.Context, accountID int, counter int, at time.Time) error {
	if _, err := s.db.ExecContext(ctx,
		"UPDATE accounts SET last_totp_counter = ?, last_totp_at = ? WHERE id = ?",
		counter, at, accountID); err != nil {
		return fmt.Errorf("set last_totp: %w", err)
	}
	return nil
}

// --- P-34: transactional helpers for the business 2FA operations ---
//
// They all take an *sql.Tx, so a composite operation never nests a second
// transaction (SaveRecoveryCodes with its own BeginTx must not be called
// from inside one of them). Every composite writes its security event as
// the last business step before Commit.

// securityEventFailureLine builds the single log line for a failed
// security-event write. It is a pure function so a test can prove that the
// line carries no token, token hash, session id, recovery code, TOTP secret,
// raw IP or database URL, and no driver text.
func securityEventFailureLine(operation, eventType string, accountID *int) string {
	aid := "none"
	if accountID != nil {
		aid = strconv.Itoa(*accountID)
	}
	return "security_event_write_failed operation=" + operation +
		" event_type=" + eventType +
		" account_id=" + aid +
		" stage=event_write error_class=db_error"
}

// parentalHistoryFailureLine builds the single WARN line for a lost
// best-effort Parental history write (P-35). Like
// securityEventFailureLine it is a pure function and takes NO error value, so
// a test can prove the line carries no driver text, no PIN, no e-mail address,
// no token, session id or raw IP, and no setting/old_value/new_value.
//
// stage is set by the CALLING HELPER and is what distinguishes the two
// history targets: stage=event_write is the failed write to security_events,
// stage=notification_write is the failed write to parental_notifications. No
// extra target field is needed.
func parentalHistoryFailureLine(eventType string, accountID int, stage string) string {
	return "WARN parental_history_write_failed event_type=" + eventType +
		" account_id=" + strconv.Itoa(accountID) +
		" stage=" + stage + " error_class=db_error"
}

// insertSecurityEventTx writes one security_events row inside tx. It is the
// ONLY place that logs security_event_write_failed, so exactly one line is
// written per failed operation (P-34).
func insertSecurityEventTx(ctx context.Context, tx *sql.Tx, eventType string, accountID *int, operation string) error {
	if _, err := tx.ExecContext(ctx,
		"INSERT INTO security_events (event_type, account_id, created_at) VALUES (?, ?, NOW())",
		eventType, accountID); err != nil {
		log.Print(securityEventFailureLine(operation, eventType, accountID))
		return fmt.Errorf("record security event: %w", err)
	}
	return nil
}

// deleteRecoveryCodesTx drops every stored recovery code of an account.
func deleteRecoveryCodesTx(ctx context.Context, tx *sql.Tx, accountID int) error {
	if _, err := tx.ExecContext(ctx,
		"DELETE FROM recovery_codes WHERE account_id = ?", accountID); err != nil {
		return fmt.Errorf("clear recovery codes: %w", err)
	}
	return nil
}

// saveRecoveryCodesTx replaces the account's recovery codes inside tx.
func saveRecoveryCodesTx(ctx context.Context, tx *sql.Tx, accountID int, codes []string) error {
	now := time.Now()
	for _, c := range codes {
		if _, err := tx.ExecContext(ctx,
			`INSERT INTO recovery_codes (token_hash, account_id, created_at)
			 VALUES (?, ?, ?)`,
			hashRecoveryCode(c), accountID, now); err != nil {
			return fmt.Errorf("insert recovery code: %w", err)
		}
	}
	return nil
}

// setTwoFactorEnabledTx flips the account flag inside tx.
func setTwoFactorEnabledTx(ctx context.Context, tx *sql.Tx, accountID int, enabled bool) error {
	b := 0
	if enabled {
		b = 1
	}
	if _, err := tx.ExecContext(ctx,
		"UPDATE accounts SET two_factor_enabled = ? WHERE id = ?", b, accountID); err != nil {
		return fmt.Errorf("set two_factor_enabled: %w", err)
	}
	return nil
}

// clearTwoFactorSecretTx drops the encrypted TOTP secret and the replay
// counter inside tx.
func clearTwoFactorSecretTx(ctx context.Context, tx *sql.Tx, accountID int) error {
	if _, err := tx.ExecContext(ctx,
		"UPDATE accounts SET two_factor_secret = NULL, last_totp_counter = NULL, last_totp_at = NULL WHERE id = ?",
		accountID); err != nil {
		return fmt.Errorf("clear two_factor_secret: %w", err)
	}
	return nil
}

// revokeAllSessionsTx marks every live session of the account revoked inside
// tx. It stays event-less: the event belongs to the calling operation
// (docs/Auth_API_Architektur.md §18).
func revokeAllSessionsTx(ctx context.Context, tx *sql.Tx, accountID int) error {
	if _, err := tx.ExecContext(ctx,
		`UPDATE sessions SET revoked_at = NOW()
		 WHERE account_id = ? AND revoked_at IS NULL`, accountID); err != nil {
		return fmt.Errorf("revoke all sessions: %w", err)
	}
	return nil
}

// revokeAllTrustedDevicesTx deletes every device of the account inside tx.
// It stays event-less for the same reason.
func revokeAllTrustedDevicesTx(ctx context.Context, tx *sql.Tx, accountID int) error {
	if _, err := tx.ExecContext(ctx,
		"DELETE FROM trusted_devices WHERE account_id = ?", accountID); err != nil {
		return fmt.Errorf("revoke all trusted devices: %w", err)
	}
	return nil
}

// SetupTwoFactor stores the secret, issues the recovery codes and enables 2FA
// in ONE transaction and records two_factor_enabled. Any failing step rolls
// everything back, so the manual compensating calls the handler used to
// issue are no longer needed (P-34).
//
// The first statement is the gate: it only fires while 2FA is not enabled.
// If two setups race, exactly one commits; the loser changed nothing and
// gets ErrTwoFactorAlreadySetUp, which the handler maps to the existing 409.
func (s *sQLStore) SetupTwoFactor(ctx context.Context, accountID int, encryptedSecret []byte, codes []string) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin setup two_factor: %w", err)
	}
	res, err := tx.ExecContext(ctx,
		"UPDATE accounts SET two_factor_enabled = 1 WHERE id = ? AND two_factor_enabled = 0", accountID)
	if err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("setup two_factor gate: %w", err)
	}
	if n, err := res.RowsAffected(); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("setup two_factor gate: %w", err)
	} else if n == 0 {
		_ = tx.Rollback()
		return ErrTwoFactorAlreadySetUp
	}
	if _, err := tx.ExecContext(ctx,
		"UPDATE accounts SET two_factor_secret = ?, last_totp_counter = NULL, last_totp_at = NULL WHERE id = ?",
		encryptedSecret, accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("setup two_factor secret: %w", err)
	}
	if err := saveRecoveryCodesTx(ctx, tx, accountID, codes); err != nil {
		_ = tx.Rollback()
		return err
	}
	if err := insertSecurityEventTx(ctx, tx, eventTwoFactorEnabled, &accountID, "setup_two_factor"); err != nil {
		_ = tx.Rollback()
		return err
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit setup two_factor: %w", err)
	}
	return nil
}

// EnableTwoFactor decides the disabled -> enabled transition INSIDE the
// transaction: the conditional update only fires while 2FA is off, so two
// racing enables produce at most one transition and at most one
// two_factor_enabled event. A handler-side pre-read is only an optimisation;
// it is not the security decision.
func (s *sQLStore) EnableTwoFactor(ctx context.Context, accountID int) (bool, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return false, fmt.Errorf("begin enable two_factor: %w", err)
	}
	res, err := tx.ExecContext(ctx,
		"UPDATE accounts SET two_factor_enabled = 1 WHERE id = ? AND two_factor_enabled = 0", accountID)
	if err != nil {
		_ = tx.Rollback()
		return false, fmt.Errorf("enable two_factor: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		_ = tx.Rollback()
		return false, fmt.Errorf("enable two_factor: %w", err)
	}
	if n == 0 {
		// Already enabled: a no-op, no event. Still commit so the
		// transaction ends cleanly.
		if err := tx.Commit(); err != nil {
			return false, fmt.Errorf("commit enable two_factor: %w", err)
		}
		return false, nil
	}
	if err := insertSecurityEventTx(ctx, tx, eventTwoFactorEnabled, &accountID, "enable_two_factor"); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := tx.Commit(); err != nil {
		return false, fmt.Errorf("commit enable two_factor: %w", err)
	}
	return true, nil
}

// DisableTwoFactor decides the enabled -> disabled transition INSIDE the
// transaction, then clears the secret, the recovery codes and every trusted
// device and records two_factor_disabled — all in one transaction.
//
// Sessions deliberately stay active (per the current revocation policy):
// no RevokeAllSessions, no session_revoked. A no-op on an already disabled
// account changes nothing and writes no event.
func (s *sQLStore) DisableTwoFactor(ctx context.Context, accountID int) (bool, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return false, fmt.Errorf("begin disable two_factor: %w", err)
	}
	res, err := tx.ExecContext(ctx,
		"UPDATE accounts SET two_factor_enabled = 0 WHERE id = ? AND two_factor_enabled = 1", accountID)
	if err != nil {
		_ = tx.Rollback()
		return false, fmt.Errorf("disable two_factor: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		_ = tx.Rollback()
		return false, fmt.Errorf("disable two_factor: %w", err)
	}
	if n == 0 {
		if err := tx.Commit(); err != nil {
			return false, fmt.Errorf("commit disable two_factor: %w", err)
		}
		return false, nil
	}
	if err := clearTwoFactorSecretTx(ctx, tx, accountID); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := deleteRecoveryCodesTx(ctx, tx, accountID); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := revokeAllTrustedDevicesTx(ctx, tx, accountID); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := insertSecurityEventTx(ctx, tx, eventTwoFactorDisabled, &accountID, "disable_two_factor"); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := tx.Commit(); err != nil {
		return false, fmt.Errorf("commit disable two_factor: %w", err)
	}
	return true, nil
}

// ResetTwoFactor rotates the whole 2FA state in ONE transaction: new secret,
// new recovery codes, 2FA enabled, every session revoked, every trusted
// device revoked, and two_factor_reset recorded. A failure at any step
// leaves the previous secret, the previous codes, the previous sessions and
// the previous devices untouched (P-34).
//
// The first statement is a compare-and-swap on the stored secret that the
// caller read at request start. Only the first of two requests that read the
// SAME stored secret gets changed=true; the loser gets changed=false with a
// nil error and has written nothing at all — no secret, no codes, no session
// revocation, no device revocation, no event — and the handler answers 409
// without any provisioning URI or recovery codes.
//
// The CAS protects the same READ starting state only. A reset that starts
// after the previous commit read the already rotated secret, is a new valid
// operation and commits normally: last commit wins. The CAS is no guarantee
// about how long an HTTP response takes to reach its client.
func (s *sQLStore) ResetTwoFactor(ctx context.Context, accountID int, expectedSecret, newSecret []byte, codes []string) (bool, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return false, fmt.Errorf("begin reset two_factor: %w", err)
	}
	// <=> is the NULL-safe equal operator: it also matches while
	// two_factor_secret IS NULL, so a first-ever setup CASes correctly.
	res, err := tx.ExecContext(ctx,
		`UPDATE accounts
		    SET two_factor_secret = ?, last_totp_counter = NULL, last_totp_at = NULL
		  WHERE id = ? AND two_factor_secret <=> ?`,
		newSecret, accountID, expectedSecret)
	if err != nil {
		_ = tx.Rollback()
		return false, fmt.Errorf("reset two_factor cas: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		_ = tx.Rollback()
		return false, fmt.Errorf("reset two_factor cas: %w", err)
	}
	if n == 0 {
		_ = tx.Rollback()
		return false, nil
	}
	if err := deleteRecoveryCodesTx(ctx, tx, accountID); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := saveRecoveryCodesTx(ctx, tx, accountID, codes); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := setTwoFactorEnabledTx(ctx, tx, accountID, true); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := revokeAllSessionsTx(ctx, tx, accountID); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := revokeAllTrustedDevicesTx(ctx, tx, accountID); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := insertSecurityEventTx(ctx, tx, eventTwoFactorReset, &accountID, "reset_two_factor"); err != nil {
		_ = tx.Rollback()
		return false, err
	}
	if err := tx.Commit(); err != nil {
		return false, fmt.Errorf("commit reset two_factor: %w", err)
	}
	return true, nil
}

// ChangePasswordRevokeAll performs the full revocation policy in one
// transaction: new password hash, revoke all sessions, revoke all
// trusted devices, and record the security event (password_changed).
// A rollback aborts the entire change.
func (s *sQLStore) ChangePasswordRevokeAll(ctx context.Context, accountID int, newPasswordHash string) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin change password: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		"UPDATE accounts SET password_hash = ? WHERE id = ?", newPasswordHash, accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("set password: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		`UPDATE sessions SET revoked_at = NOW()
		 WHERE account_id = ? AND revoked_at IS NULL`, accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("revoke sessions: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		"DELETE FROM trusted_devices WHERE account_id = ?", accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("revoke trusted devices: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		"INSERT INTO security_events (event_type, account_id, created_at) VALUES (?, ?, NOW())",
		eventPasswordChanged, accountID); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("record security event: %w", err)
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit change password: %w", err)
	}
	return nil
}

// CountTrustedDevices counts only ACTIVE (not yet expired) devices so
// an expired one frees its slot in the 3-device limit.
func (s *sQLStore) CountTrustedDevices(ctx context.Context, accountID int) (int, error) {
	var n int
	if err := s.db.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM trusted_devices
		 WHERE account_id = ? AND last_used_at >= NOW() - INTERVAL 30 DAY`, accountID).Scan(&n); err != nil {
		return 0, fmt.Errorf("count trusted devices: %w", err)
	}
	return n, nil
}

// RevokeAllSessions marks every live session of the account as explicitly
// revoked (rows are kept, see migration 014).
func (s *sQLStore) RevokeAllSessions(ctx context.Context, accountID int) error {
	if _, err := s.db.ExecContext(ctx,
		`UPDATE sessions SET revoked_at = NOW()
		 WHERE account_id = ? AND revoked_at IS NULL`, accountID); err != nil {
		return fmt.Errorf("revoke all sessions: %w", err)
	}
	return nil
}

func (s *sQLStore) AddTrustedDevice(ctx context.Context, accountID int, rawToken, label string) (TrustedDevice, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return TrustedDevice{}, fmt.Errorf("begin add trusted device: %w", err)
	}
	// Expired devices no longer count against the limit and their rows
	// are purged here (within the same transaction) so a re-presented
	// token after expiry cannot collide with the unique token_hash key
	// left by an expired row.
	if _, err := tx.ExecContext(ctx,
		`DELETE FROM trusted_devices
		 WHERE account_id = ? AND last_used_at < NOW() - INTERVAL 30 DAY`, accountID); err != nil {
		_ = tx.Rollback()
		return TrustedDevice{}, fmt.Errorf("purge expired devices: %w", err)
	}
	var n int
	if err := tx.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM trusted_devices
		 WHERE account_id = ? AND last_used_at >= NOW() - INTERVAL 30 DAY`, accountID).Scan(&n); err != nil {
		_ = tx.Rollback()
		return TrustedDevice{}, fmt.Errorf("count trusted devices: %w", err)
	}
	if n >= maxTrustedDevices {
		_ = tx.Rollback()
		return TrustedDevice{}, ErrMaxDevices
	}
	now := time.Now()
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO trusted_devices (account_id, token_hash, label, confirmed_at, created_at, last_used_at)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		accountID, tokenHash(rawToken), label, now, now, now); err != nil {
		_ = tx.Rollback()
		return TrustedDevice{}, fmt.Errorf("insert trusted device: %w", err)
	}
	var id int
	if err := tx.QueryRowContext(ctx,
		"SELECT id FROM trusted_devices WHERE account_id = ? AND token_hash = ?",
		accountID, tokenHash(rawToken)).Scan(&id); err != nil {
		_ = tx.Rollback()
		return TrustedDevice{}, fmt.Errorf("lookup trusted device: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		"INSERT INTO security_events (event_type, account_id, created_at) VALUES (?, ?, NOW())",
		eventDeviceConfirmed, accountID); err != nil {
		_ = tx.Rollback()
		return TrustedDevice{}, fmt.Errorf("record security event: %w", err)
	}
	if err := tx.Commit(); err != nil {
		return TrustedDevice{}, fmt.Errorf("commit add trusted device: %w", err)
	}
	return TrustedDevice{
		ID:          id,
		AccountID:   accountID,
		TokenHash:   tokenHash(rawToken),
		Label:       label,
		ConfirmedAt: now,
		CreatedAt:   now,
		LastUsedAt:  now,
	}, nil
}

// HasTrustedDevice reports whether a CONFIRMED AND ACTIVE device
// matches the presented token. An expired device (last used more than
// 30 days ago) is not recognized, so it does not bypass 2FA.
func (s *sQLStore) HasTrustedDevice(ctx context.Context, accountID int, rawToken string) (bool, error) {
	var lastUsed time.Time
	err := s.db.QueryRowContext(ctx,
		"SELECT last_used_at FROM trusted_devices WHERE account_id = ? AND token_hash = ?",
		accountID, tokenHash(rawToken)).Scan(&lastUsed)
	if err == sql.ErrNoRows {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("lookup trusted device: %w", err)
	}
	return trustedDeviceActive(lastUsed, time.Now()), nil
}

// TouchTrustedDevice updates last_used_at of a confirmed device after a
// successful login with that device. Returns sql.ErrNoRows when the
// device is unknown for the account.
func (s *sQLStore) TouchTrustedDevice(ctx context.Context, accountID int, rawToken string) error {
	res, err := s.db.ExecContext(ctx,
		"UPDATE trusted_devices SET last_used_at = NOW() WHERE account_id = ? AND token_hash = ?",
		accountID, tokenHash(rawToken))
	if err != nil {
		return fmt.Errorf("touch trusted device: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("touch trusted device: %w", err)
	}
	return errWrapNoRows(n)
}

func (s *sQLStore) RevokeTrustedDevice(ctx context.Context, accountID int, rawToken string) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin revoke trusted device: %w", err)
	}
	res, err := tx.ExecContext(ctx,
		"DELETE FROM trusted_devices WHERE account_id = ? AND token_hash = ?",
		accountID, tokenHash(rawToken))
	if err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("revoke trusted device: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("revoke trusted device: %w", err)
	}
	if n == 0 {
		_ = tx.Rollback()
		return sql.ErrNoRows
	}
	if err := insertSecurityEventTx(ctx, tx, eventDeviceRevoked, &accountID, "revoke_trusted_device"); err != nil {
		_ = tx.Rollback()
		return err
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit revoke trusted device: %w", err)
	}
	return nil
}

func (s *sQLStore) ListTrustedDevices(ctx context.Context, accountID int) ([]TrustedDevice, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT id, account_id, token_hash, label, confirmed_at, created_at, last_used_at
		 FROM trusted_devices WHERE account_id = ? ORDER BY id`, accountID)
	if err != nil {
		return nil, fmt.Errorf("list trusted devices: %w", err)
	}
	defer rows.Close()
	out := []TrustedDevice{}
	for rows.Next() {
		var d TrustedDevice
		if err := rows.Scan(&d.ID, &d.AccountID, &d.TokenHash, &d.Label, &d.ConfirmedAt, &d.CreatedAt, &d.LastUsedAt); err != nil {
			return nil, fmt.Errorf("scan trusted device: %w", err)
		}
		out = append(out, d)
	}
	return out, rows.Err()
}

func (s *sQLStore) RevokeAllTrustedDevices(ctx context.Context, accountID int) error {
	if _, err := s.db.ExecContext(ctx,
		"DELETE FROM trusted_devices WHERE account_id = ?", accountID); err != nil {
		return fmt.Errorf("revoke all trusted devices: %w", err)
	}
	return nil
}

func (s *sQLStore) SaveRecoveryCodes(ctx context.Context, accountID int, codes []string) error {
	if len(codes) == 0 {
		return nil
	}
	now := time.Now()
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin save recovery codes: %w", err)
	}
	for _, c := range codes {
		if _, err := tx.ExecContext(ctx,
			`INSERT INTO recovery_codes (token_hash, account_id, created_at)
			 VALUES (?, ?, ?)`,
			hashRecoveryCode(c), accountID, now); err != nil {
			_ = tx.Rollback()
			return fmt.Errorf("insert recovery code: %w", err)
		}
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit save recovery codes: %w", err)
	}
	return nil
}

// UseRecoveryCode marks the presented recovery code used (single-use) and
// records recovery_code_used in the SAME transaction: either the code is
// consumed AND the event exists, or neither happened (P-34). An event
// failure therefore leaves the code usable again.
func (s *sQLStore) UseRecoveryCode(ctx context.Context, accountID int, rawCode string) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin use recovery code: %w", err)
	}
	res, err := tx.ExecContext(ctx,
		`UPDATE recovery_codes SET used_at = NOW()
		 WHERE account_id = ? AND token_hash = ? AND used_at IS NULL`,
		accountID, hashRecoveryCode(rawCode))
	if err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("use recovery code: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("use recovery code: %w", err)
	}
	if n == 0 {
		_ = tx.Rollback()
		return sql.ErrNoRows
	}
	if err := insertSecurityEventTx(ctx, tx, eventRecoveryCodeUsed, &accountID, "use_recovery_code"); err != nil {
		_ = tx.Rollback()
		return err
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit use recovery code: %w", err)
	}
	return nil
}

func (s *sQLStore) ClearRecoveryCodes(ctx context.Context, accountID int) error {
	if _, err := s.db.ExecContext(ctx,
		"DELETE FROM recovery_codes WHERE account_id = ?", accountID); err != nil {
		return fmt.Errorf("clear recovery codes: %w", err)
	}
	return nil
}

// CountRecoveryCodes returns how many (unused) recovery codes the
// account still has.
func (s *sQLStore) CountRecoveryCodes(ctx context.Context, accountID int) (int, error) {
	var n int
	if err := s.db.QueryRowContext(ctx,
		"SELECT COUNT(*) FROM recovery_codes WHERE account_id = ? AND used_at IS NULL", accountID).Scan(&n); err != nil {
		return 0, fmt.Errorf("count recovery codes: %w", err)
	}
	return n, nil
}

func (s *sQLStore) RecordSecurityEvent(ctx context.Context, eventType string, accountID *int) error {
	if _, err := s.db.ExecContext(ctx,
		"INSERT INTO security_events (event_type, account_id, created_at) VALUES (?, ?, NOW())",
		eventType, accountID); err != nil {
		return fmt.Errorf("record security event: %w", err)
	}
	return nil
}

func (s *sQLStore) ListSecurityEvents(ctx context.Context, accountID int) ([]SecurityEvent, error) {
	rows, err := s.db.QueryContext(ctx,
		"SELECT id, event_type, account_id, created_at FROM security_events WHERE account_id = ? ORDER BY id DESC",
		accountID)
	if err != nil {
		return nil, fmt.Errorf("list security events: %w", err)
	}
	defer rows.Close()
	out := []SecurityEvent{}
	for rows.Next() {
		var e SecurityEvent
		if err := rows.Scan(&e.ID, &e.EventType, &e.AccountID, &e.CreatedAt); err != nil {
			return nil, fmt.Errorf("scan security event: %w", err)
		}
		out = append(out, e)
	}
	return out, rows.Err()
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
