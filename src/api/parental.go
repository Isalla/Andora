package main

import (
	"context"
	"database/sql"
	"fmt"
	"time"
)

// Parental control (migration 013). The control is account-bound and
// enforced server-side: the account row only carries the
// parental_control_enabled flag, the full rule set lives in the
// parental_* tables. The realm server never touches the auth DB; it
// only calls the parental endpoints with its own service credential.
//
// Conventions:
//   - day limits are minutes, 0 = unlimited (weekly rules, special
//     periods and exception overrides alike)
//   - the parent PIN is stored as Argon2id hash only (parent_pin_hash),
//     4-16 digits
//   - the parent e-mail is stored encrypted only (AES-256-GCM), no
//     reversible lookup hash
//   - notifications are DB records; there is no mail engine in this
//     service, the calling service delivers them

const (
	// parentalExtensionSeconds is the once-per-day in-game extension
	// granted after parent PIN entry: exactly one hour.
	parentalExtensionSeconds = 3600
	// parentalPollMax caps how much play time a single status poll may
	// claim, so a reconnect after a long gap cannot consume a large
	// chunk of the budget at once.
	parentalPollMax = 120 * time.Second
)

// ErrExtensionUsed reports that today's +1h extension was already
// consumed; it is the service-level signal for the 409 response.
var ErrExtensionUsed = fmt.Errorf("parental extension already used today")

// Parental security/notification event types. They extend the
// security_events log (VARCHAR, no closed DB set) and double as the
// parental_notifications event types.
const (
	eventParentalSetup        = "parental_setup"
	eventParentalSettings     = "parental_settings"
	eventParentalRemoved      = "parental_removed"
	eventParentalPinChanged   = "parental_pin_changed"
	eventParentalEmailChanged = "parental_email_changed"
	eventParentalEmailRemoved = "parental_email_removed"
	eventParentalPeriod       = "parental_period"
	eventParentalException    = "parental_exception"
)

// weekIndexOf maps a time to the week array index: Monday=0 .. Sunday=6.
func weekIndexOf(t time.Time) int {
	if d := int(t.Weekday()); d != 0 {
		return d - 1
	}
	return 6
}

// parentalBufferDuration is the grace buffer for an already-running
// session whose budget hit zero: 15 minutes Sunday-Thursday, 30 minutes
// Friday-Saturday.
func parentalBufferDuration(now time.Time) time.Duration {
	if w := now.Weekday(); w == time.Friday || w == time.Saturday {
		return 30 * time.Minute
	}
	return 15 * time.Minute
}

// parentalDayKey truncates a time to its calendar day (server time is
// authoritative; client clocks are never trusted).
func parentalDayKey(t time.Time) time.Time {
	return time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, t.Location())
}

// parentalDayString formats a day for DATE comparisons.
func parentalDayString(t time.Time) string {
	return parentalDayKey(t).Format("2006-01-02")
}

// ParentalControls is the rule set of one supervised account.
// Week holds Monday..Sunday limits in minutes (0 = unlimited).
type ParentalControls struct {
	AccountID      int
	PinHash        string
	ParentEmailEnc []byte
	Week           [7]int
	ChatEnabled    bool
	VoiceEnabled   bool
	WarningMinutes int
	UpdatedAt      time.Time
}

// ParentalPeriod is one special (holiday) period with its own weekly
// limits. Active periods must not overlap (checked in the handler).
type ParentalPeriod struct {
	ID        int
	AccountID int
	Start     time.Time
	End       time.Time
	Week      [7]int
}

// ParentalException is one calendar-day exception: either extra minutes
// on top of the resolved limit or a full override (0 = unlimited).
type ParentalException struct {
	ID              int
	AccountID       int
	Date            time.Time
	ExtraMinutes    *int
	OverrideMinutes *int
}

// ParentalDailyUsage is the per-day server-side counter. extended_at
// persists the once-per-day +1h across logout/reconnect; buffer_started_at
// persists the grace buffer so no re-login is possible during it.
type ParentalDailyUsage struct {
	AccountID       int
	Date            time.Time
	UsedSeconds     int
	ExtendedAt      *time.Time
	BufferStartedAt *time.Time
	LastPolledAt    *time.Time
}

// ParentalNotification is one change receipt for the parent e-mail.
// recipient_email_enc is the affected address (the previous one on
// change/removal), stored encrypted; plaintext is returned exactly once
// via the deliver endpoint.
type ParentalNotification struct {
	ID                int
	AccountID         int
	EventType         string
	SettingName       string
	OldValue          string
	NewValue          string
	CreatedAt         time.Time
	RecipientEmailEnc []byte
	DeliveredAt       *time.Time
}

// parentalState is the authoritative server-computed view of a
// supervised account at one point in time.
type parentalState struct {
	Enabled           bool
	Unlimited         bool
	DayMinutes        int
	UsedSeconds       int
	RemainingSeconds  int
	WarningSeconds    int
	Warning           bool
	ExtendedUsedToday bool
	BufferActive      bool
	BufferUntil       *time.Time
	Blocked           bool
	ForceLogout       bool
	ChatAllowed       bool
	VoiceAllowed      bool
}

// computeParentalState derives the state from the auth DB. sessionActive
// means the account currently holds a live session (status polling with
// a valid session). Only a live session starts the grace buffer and
// accrues play time; login decisions use sessionActive=false. recordTime
// additionally persists accrued seconds (status endpoint only).
func (s *Server) computeParentalState(ctx context.Context, acc *Account, sessionActive, recordTime bool, now time.Time) (parentalState, error) {
	st := parentalState{}
	if acc == nil || !acc.ParentalEnabled {
		return st, nil
	}
	ctl, err := s.store.FetchParentalControls(ctx, acc.ID)
	if err != nil {
		return st, err
	}
	if ctl == nil {
		return st, nil
	}
	st.Enabled = true
	st.ChatAllowed = ctl.ChatEnabled
	st.VoiceAllowed = ctl.VoiceEnabled
	warnMin := ctl.WarningMinutes
	if warnMin < 0 {
		warnMin = 0
	}
	st.WarningSeconds = warnMin * 60

	day := parentalDayKey(now)
	limit := ctl.Week[weekIndexOf(now)]
	if p, err := s.store.GetActiveParentalPeriod(ctx, acc.ID, day); err != nil {
		return st, err
	} else if p != nil {
		limit = p.Week[weekIndexOf(now)]
	}
	if ex, err := s.store.FetchParentalException(ctx, acc.ID, day); err != nil {
		return st, err
	} else if ex != nil {
		if ex.OverrideMinutes != nil {
			limit = *ex.OverrideMinutes
		} else if ex.ExtraMinutes != nil {
			limit += *ex.ExtraMinutes
		}
	}
	if limit < 0 {
		limit = 0
	}
	st.DayMinutes = limit
	st.Unlimited = limit == 0

	usage, err := s.store.FetchParentalUsage(ctx, acc.ID, day)
	if err != nil {
		return st, err
	}
	st.UsedSeconds = usage.UsedSeconds
	extended := usage.ExtendedAt != nil
	st.ExtendedUsedToday = extended

	// Accrue play time for a live session from the last poll stamp.
	if recordTime && sessionActive && !st.Unlimited {
		if usage.LastPolledAt != nil {
			dt := now.Sub(*usage.LastPolledAt)
			if dt > 0 {
				if dt > parentalPollMax {
					dt = parentalPollMax
				}
				sec := int(dt.Seconds())
				if sec > 0 {
					if err := s.store.AddParentalUsageSeconds(ctx, acc.ID, day, sec, now, limit); err != nil {
						return st, err
					}
					usage.UsedSeconds += sec
					st.UsedSeconds = usage.UsedSeconds
				}
			}
		} else {
			if err := s.store.AddParentalUsageSeconds(ctx, acc.ID, day, 0, now, limit); err != nil {
				return st, err
			}
		}
		usage.LastPolledAt = &now
	}

	extra := 0
	if extended {
		extra = parentalExtensionSeconds
	}
	if !st.Unlimited {
		rem := limit*60 + extra - usage.UsedSeconds
		if rem < 0 {
			rem = 0
		}
		st.RemainingSeconds = rem
	}
	if !st.Unlimited && st.RemainingSeconds > 0 && st.RemainingSeconds <= st.WarningSeconds {
		st.Warning = true
	}

	// Grace buffer of an already-running session.
	bufDur := parentalBufferDuration(now)
	if usage.BufferStartedAt != nil {
		until := usage.BufferStartedAt.Add(bufDur)
		if now.Before(until) {
			st.BufferActive = true
			st.BufferUntil = &until
		} else if sessionActive && !st.Unlimited && st.RemainingSeconds == 0 {
			// The buffer has fully elapsed while the session still
			// runs: the realm must log the player out now.
			st.ForceLogout = true
		}
	}
	// A live session that exhausts the budget starts the buffer exactly
	// once per day; it persists across logout/reconnect.
	if sessionActive && !st.Unlimited && st.RemainingSeconds == 0 && usage.BufferStartedAt == nil {
		if err := s.store.SetParentalBufferStart(ctx, acc.ID, day, now); err == nil {
			until := now.Add(bufDur)
			st.BufferActive = true
			st.BufferUntil = &until
		}
	}
	// No new login while the budget is exhausted (buffer or not).
	if !st.Unlimited && st.RemainingSeconds == 0 {
		st.Blocked = true
	}
	return st, nil
}

// --- SQL store: parental control ---

const parentalColumns = `account_id, parent_pin_hash, parent_email_enc,
	monday_minutes, tuesday_minutes, wednesday_minutes, thursday_minutes,
	friday_minutes, saturday_minutes, sunday_minutes,
	chat_enabled, voice_enabled, warning_minutes, updated_at`

func scanParentalControls(row *sql.Row) (*ParentalControls, error) {
	ctl := &ParentalControls{}
	err := row.Scan(&ctl.AccountID, &ctl.PinHash, &ctl.ParentEmailEnc,
		&ctl.Week[0], &ctl.Week[1], &ctl.Week[2], &ctl.Week[3],
		&ctl.Week[4], &ctl.Week[5], &ctl.Week[6],
		&ctl.ChatEnabled, &ctl.VoiceEnabled, &ctl.WarningMinutes, &ctl.UpdatedAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return ctl, nil
}

func (s *sQLStore) FetchParentalControls(ctx context.Context, accountID int) (*ParentalControls, error) {
	ctl, err := scanParentalControls(s.db.QueryRowContext(ctx,
		"SELECT "+parentalColumns+" FROM parental_controls WHERE account_id = ?", accountID))
	if err != nil {
		return nil, fmt.Errorf("lookup parental controls: %w", err)
	}
	return ctl, nil
}

func (s *sQLStore) CreateParentalControl(ctx context.Context, ctl *ParentalControls) error {
	if _, err := s.db.ExecContext(ctx,
		`INSERT INTO parental_controls
		 (account_id, parent_pin_hash, parent_email_enc,
		  monday_minutes, tuesday_minutes, wednesday_minutes, thursday_minutes,
		  friday_minutes, saturday_minutes, sunday_minutes,
		  chat_enabled, voice_enabled, warning_minutes, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW())`,
		ctl.AccountID, ctl.PinHash, ctl.ParentEmailEnc,
		ctl.Week[0], ctl.Week[1], ctl.Week[2], ctl.Week[3],
		ctl.Week[4], ctl.Week[5], ctl.Week[6],
		boolInt(ctl.ChatEnabled), boolInt(ctl.VoiceEnabled), ctl.WarningMinutes); err != nil {
		return fmt.Errorf("insert parental controls: %w", err)
	}
	return nil
}

func (s *sQLStore) UpdateParentalControls(ctx context.Context, ctl *ParentalControls) error {
	res, err := s.db.ExecContext(ctx,
		`UPDATE parental_controls SET
		  monday_minutes = ?, tuesday_minutes = ?, wednesday_minutes = ?,
		  thursday_minutes = ?, friday_minutes = ?,
		  saturday_minutes = ?, sunday_minutes = ?,
		  chat_enabled = ?, voice_enabled = ?,
		  warning_minutes = ?, updated_at = NOW()
		 WHERE account_id = ?`,
		ctl.Week[0], ctl.Week[1], ctl.Week[2], ctl.Week[3],
		ctl.Week[4], ctl.Week[5], ctl.Week[6],
		boolInt(ctl.ChatEnabled), boolInt(ctl.VoiceEnabled),
		ctl.WarningMinutes, ctl.AccountID)
	if err != nil {
		return fmt.Errorf("update parental controls: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("update parental controls: %w", err)
	}
	return errWrapNoRows(n)
}

func (s *sQLStore) SetParentPinHash(ctx context.Context, accountID int, hash string) error {
	res, err := s.db.ExecContext(ctx,
		"UPDATE parental_controls SET parent_pin_hash = ?, updated_at = NOW() WHERE account_id = ?",
		hash, accountID)
	if err != nil {
		return fmt.Errorf("set parent pin: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("set parent pin: %w", err)
	}
	return errWrapNoRows(n)
}

func (s *sQLStore) SetParentEmailEnc(ctx context.Context, accountID int, enc []byte) error {
	res, err := s.db.ExecContext(ctx,
		"UPDATE parental_controls SET parent_email_enc = ?, updated_at = NOW() WHERE account_id = ?",
		enc, accountID)
	if err != nil {
		return fmt.Errorf("set parent email: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("set parent email: %w", err)
	}
	return errWrapNoRows(n)
}

func (s *sQLStore) SetParentalEnabled(ctx context.Context, accountID int, enabled bool) error {
	if _, err := s.db.ExecContext(ctx,
		"UPDATE accounts SET parental_control_enabled = ? WHERE id = ?",
		boolInt(enabled), accountID); err != nil {
		return fmt.Errorf("set parental enabled: %w", err)
	}
	return nil
}

// DeleteParentalControls removes the control row plus periods,
// exceptions and daily usage of the account. Notifications survive
// (they reference accounts, and the removal receipt must stay
// deliverable); the caller disables the account flag separately.
func (s *sQLStore) DeleteParentalControls(ctx context.Context, accountID int) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin delete parental: %w", err)
	}
	for _, q := range []string{
		"DELETE FROM parental_control_periods WHERE account_id = ?",
		"DELETE FROM parental_control_exceptions WHERE account_id = ?",
		"DELETE FROM parental_daily_usage WHERE account_id = ?",
		"DELETE FROM parental_controls WHERE account_id = ?",
	} {
		if _, err := tx.ExecContext(ctx, q, accountID); err != nil {
			_ = tx.Rollback()
			return fmt.Errorf("delete parental: %w", err)
		}
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit delete parental: %w", err)
	}
	return nil
}

func boolInt(b bool) int {
	if b {
		return 1
	}
	return 0
}

func scanParentalPeriod(row *sql.Row) (*ParentalPeriod, error) {
	p := &ParentalPeriod{}
	err := row.Scan(&p.ID, &p.AccountID, &p.Start, &p.End,
		&p.Week[0], &p.Week[1], &p.Week[2], &p.Week[3],
		&p.Week[4], &p.Week[5], &p.Week[6])
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return p, nil
}

const parentalPeriodColumns = `id, account_id, starts_at, ends_at,
	monday_minutes, tuesday_minutes, wednesday_minutes, thursday_minutes,
	friday_minutes, saturday_minutes, sunday_minutes`

func (s *sQLStore) ListParentalPeriods(ctx context.Context, accountID int) ([]ParentalPeriod, error) {
	rows, err := s.db.QueryContext(ctx,
		"SELECT "+parentalPeriodColumns+" FROM parental_control_periods WHERE account_id = ? ORDER BY starts_at",
		accountID)
	if err != nil {
		return nil, fmt.Errorf("list parental periods: %w", err)
	}
	defer rows.Close()
	out := []ParentalPeriod{}
	for rows.Next() {
		var p ParentalPeriod
		if err := rows.Scan(&p.ID, &p.AccountID, &p.Start, &p.End,
			&p.Week[0], &p.Week[1], &p.Week[2], &p.Week[3],
			&p.Week[4], &p.Week[5], &p.Week[6]); err != nil {
			return nil, fmt.Errorf("scan parental period: %w", err)
		}
		out = append(out, p)
	}
	return out, rows.Err()
}

func (s *sQLStore) AddParentalPeriod(ctx context.Context, p *ParentalPeriod) (int, error) {
	res, err := s.db.ExecContext(ctx,
		`INSERT INTO parental_control_periods
		 (account_id, starts_at, ends_at,
		  monday_minutes, tuesday_minutes, wednesday_minutes, thursday_minutes,
		  friday_minutes, saturday_minutes, sunday_minutes)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		p.AccountID, parentalDayString(p.Start), parentalDayString(p.End),
		p.Week[0], p.Week[1], p.Week[2], p.Week[3],
		p.Week[4], p.Week[5], p.Week[6])
	if err != nil {
		return 0, fmt.Errorf("insert parental period: %w", err)
	}
	id, err := res.LastInsertId()
	if err != nil {
		return 0, fmt.Errorf("insert parental period id: %w", err)
	}
	return int(id), nil
}

func (s *sQLStore) DeleteParentalPeriod(ctx context.Context, accountID, id int) error {
	res, err := s.db.ExecContext(ctx,
		"DELETE FROM parental_control_periods WHERE account_id = ? AND id = ?", accountID, id)
	if err != nil {
		return fmt.Errorf("delete parental period: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("delete parental period: %w", err)
	}
	return errWrapNoRows(n)
}

// GetActiveParentalPeriod returns the period covering date, or nil.
// Overlapping active periods are rejected at insert time, so at most
// one row can match.
func (s *sQLStore) GetActiveParentalPeriod(ctx context.Context, accountID int, date time.Time) (*ParentalPeriod, error) {
	p, err := scanParentalPeriod(s.db.QueryRowContext(ctx,
		"SELECT "+parentalPeriodColumns+" FROM parental_control_periods WHERE account_id = ? AND starts_at <= ? AND ends_at >= ? LIMIT 1",
		accountID, parentalDayString(date), parentalDayString(date)))
	if err != nil {
		return nil, fmt.Errorf("lookup parental period: %w", err)
	}
	return p, nil
}

func scanParentalException(row *sql.Row) (*ParentalException, error) {
	e := &ParentalException{}
	err := row.Scan(&e.ID, &e.AccountID, &e.Date, &e.ExtraMinutes, &e.OverrideMinutes)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return e, nil
}

func (s *sQLStore) FetchParentalException(ctx context.Context, accountID int, date time.Time) (*ParentalException, error) {
	e, err := scanParentalException(s.db.QueryRowContext(ctx,
		`SELECT id, account_id, exception_date, extra_minutes, override_minutes
		 FROM parental_control_exceptions WHERE account_id = ? AND exception_date = ?`,
		accountID, parentalDayString(date)))
	if err != nil {
		return nil, fmt.Errorf("lookup parental exception: %w", err)
	}
	return e, nil
}

func (s *sQLStore) SaveParentalException(ctx context.Context, e *ParentalException) (int, error) {
	res, err := s.db.ExecContext(ctx,
		`INSERT INTO parental_control_exceptions (account_id, exception_date, extra_minutes, override_minutes)
		 VALUES (?, ?, ?, ?)
		 ON DUPLICATE KEY UPDATE extra_minutes = VALUES(extra_minutes), override_minutes = VALUES(override_minutes)`,
		e.AccountID, parentalDayString(e.Date), e.ExtraMinutes, e.OverrideMinutes)
	if err != nil {
		return 0, fmt.Errorf("save parental exception: %w", err)
	}
	id, err := res.LastInsertId()
	if err != nil || id == 0 {
		// Row already existed: resolve its id.
		cur, ferr := s.FetchParentalException(ctx, e.AccountID, e.Date)
		if ferr != nil {
			return 0, ferr
		}
		if cur == nil {
			return 0, fmt.Errorf("save parental exception: lost row")
		}
		return cur.ID, nil
	}
	return int(id), nil
}

func (s *sQLStore) DeleteParentalException(ctx context.Context, accountID int, date time.Time) error {
	res, err := s.db.ExecContext(ctx,
		"DELETE FROM parental_control_exceptions WHERE account_id = ? AND exception_date = ?",
		accountID, parentalDayString(date))
	if err != nil {
		return fmt.Errorf("delete parental exception: %w", err)
	}
	n, err := res.RowsAffected()
	if err != nil {
		return fmt.Errorf("delete parental exception: %w", err)
	}
	return errWrapNoRows(n)
}

func (s *sQLStore) FetchParentalUsage(ctx context.Context, accountID int, date time.Time) (ParentalDailyUsage, error) {
	u := ParentalDailyUsage{AccountID: accountID, Date: parentalDayKey(date)}
	row := s.db.QueryRowContext(ctx,
		`SELECT used_seconds, extended_at, buffer_started_at, last_poll_at
		 FROM parental_daily_usage WHERE account_id = ? AND usage_date = ?`,
		accountID, parentalDayString(date))
	err := row.Scan(&u.UsedSeconds, &u.ExtendedAt, &u.BufferStartedAt, &u.LastPolledAt)
	if err == sql.ErrNoRows {
		return u, nil
	}
	if err != nil {
		return u, fmt.Errorf("lookup parental usage: %w", err)
	}
	return u, nil
}

// AddParentalUsageSeconds accrues played seconds for the day (upsert);
// lastPolled stamps the poll so the next call only claims the delta.
func (s *sQLStore) AddParentalUsageSeconds(ctx context.Context, accountID int, date time.Time, seconds int, lastPolled time.Time, dayLimit int) error {
	if _, err := s.db.ExecContext(ctx,
		`INSERT INTO parental_daily_usage
		 (account_id, usage_date, used_seconds, last_poll_at, day_minutes_limit)
		 VALUES (?, ?, ?, ?, ?)
		 ON DUPLICATE KEY UPDATE
		   used_seconds = used_seconds + VALUES(used_seconds),
		   last_poll_at = VALUES(last_poll_at),
		   day_minutes_limit = VALUES(day_minutes_limit)`,
		accountID, parentalDayString(date), seconds, lastPolled, dayLimit); err != nil {
		return fmt.Errorf("accrue parental usage: %w", err)
	}
	return nil
}

// SetParentalBufferStart stamps the grace buffer exactly once per day:
// concurrent polls keep the first stamp (WHERE buffer_started_at IS NULL).
func (s *sQLStore) SetParentalBufferStart(ctx context.Context, accountID int, date time.Time, t time.Time) error {
	res, err := s.db.ExecContext(ctx,
		`INSERT INTO parental_daily_usage (account_id, usage_date, buffer_started_at, last_poll_at, day_minutes_limit)
		 VALUES (?, ?, ?, ?, 0)
		 ON DUPLICATE KEY UPDATE buffer_started_at = COALESCE(buffer_started_at, VALUES(buffer_started_at))`,
		accountID, parentalDayString(date), t, t)
	if err != nil {
		return fmt.Errorf("start parental buffer: %w", err)
	}
	_ = res
	return nil
}

// UseParentalExtension consumes the once-per-day +1h. A second call for
// the same day returns ErrExtensionUsed.
func (s *sQLStore) UseParentalExtension(ctx context.Context, accountID int, date time.Time, t time.Time) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin parental extension: %w", err)
	}
	var ext sql.NullTime
	err = tx.QueryRowContext(ctx,
		"SELECT extended_at FROM parental_daily_usage WHERE account_id = ? AND usage_date = ?",
		accountID, parentalDayString(date)).Scan(&ext)
	if err != nil && err != sql.ErrNoRows {
		_ = tx.Rollback()
		return fmt.Errorf("lookup parental extension: %w", err)
	}
	if err == nil && ext.Valid {
		_ = tx.Rollback()
		return ErrExtensionUsed
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO parental_daily_usage (account_id, usage_date, extended_at, last_poll_at, day_minutes_limit)
		 VALUES (?, ?, ?, ?, 0)
		 ON DUPLICATE KEY UPDATE extended_at = COALESCE(extended_at, VALUES(extended_at))`,
		accountID, parentalDayString(date), t, t); err != nil {
		_ = tx.Rollback()
		return fmt.Errorf("use parental extension: %w", err)
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit parental extension: %w", err)
	}
	return nil
}

func (s *sQLStore) CreateParentalNotification(ctx context.Context, n *ParentalNotification) (int, error) {
	res, err := s.db.ExecContext(ctx,
		`INSERT INTO parental_notifications
		 (account_id, event_type, setting_name, old_value, new_value, recipient_email_enc)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		n.AccountID, n.EventType, n.SettingName, n.OldValue, n.NewValue, n.RecipientEmailEnc)
	if err != nil {
		return 0, fmt.Errorf("insert parental notification: %w", err)
	}
	id, err := res.LastInsertId()
	if err != nil {
		return 0, fmt.Errorf("insert parental notification id: %w", err)
	}
	return int(id), nil
}

func (s *sQLStore) ListPendingParentalNotifications(ctx context.Context, accountID int) ([]ParentalNotification, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT id, account_id, event_type, setting_name, old_value, new_value, created_at, recipient_email_enc, delivered_at
		 FROM parental_notifications WHERE account_id = ? AND delivered_at IS NULL ORDER BY id`,
		accountID)
	if err != nil {
		return nil, fmt.Errorf("list parental notifications: %w", err)
	}
	defer rows.Close()
	out := []ParentalNotification{}
	for rows.Next() {
		var n ParentalNotification
		if err := rows.Scan(&n.ID, &n.AccountID, &n.EventType, &n.SettingName,
			&n.OldValue, &n.NewValue, &n.CreatedAt, &n.RecipientEmailEnc, &n.DeliveredAt); err != nil {
			return nil, fmt.Errorf("scan parental notification: %w", err)
		}
		out = append(out, n)
	}
	return out, rows.Err()
}

// MarkParentalNotificationsDelivered marks the given pending rows
// delivered and returns them (encrypted recipient included) so the
// handler can return the plaintext address exactly once.
func (s *sQLStore) MarkParentalNotificationsDelivered(ctx context.Context, accountID int, ids []int, t time.Time) ([]ParentalNotification, error) {
	if len(ids) == 0 {
		return nil, nil
	}
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("begin deliver notifications: %w", err)
	}
	out := []ParentalNotification{}
	for _, id := range ids {
		var n ParentalNotification
		err := tx.QueryRowContext(ctx,
			`SELECT id, account_id, event_type, setting_name, old_value, new_value, created_at, recipient_email_enc, delivered_at
			 FROM parental_notifications WHERE id = ? AND account_id = ? AND delivered_at IS NULL`,
			id, accountID).Scan(&n.ID, &n.AccountID, &n.EventType, &n.SettingName,
			&n.OldValue, &n.NewValue, &n.CreatedAt, &n.RecipientEmailEnc, &n.DeliveredAt)
		if err == sql.ErrNoRows {
			continue
		}
		if err != nil {
			_ = tx.Rollback()
			return nil, fmt.Errorf("lookup parental notification: %w", err)
		}
		if _, err := tx.ExecContext(ctx,
			"UPDATE parental_notifications SET delivered_at = ? WHERE id = ?", t, id); err != nil {
			_ = tx.Rollback()
			return nil, fmt.Errorf("deliver parental notification: %w", err)
		}
		d := t
		n.DeliveredAt = &d
		out = append(out, n)
	}
	if err := tx.Commit(); err != nil {
		return nil, fmt.Errorf("commit deliver notifications: %w", err)
	}
	return out, nil
}
