package main

import (
	"time"

	_ "github.com/go-sql-driver/mysql"
)

// Account holds only the non-sensitive data needed to check a login.
// password_hash is used internally for the check and is never
// returned over the API.
//
// TwoFactorSecret holds the ENCRYPTED (AES-256-GCM) TOTP secret; the
// plaintext Base32 secret (20 random bytes) is never stored and only
// exists in memory during setup / the provisioning URI.
type Account struct {
	ID           int
	Username     string
	PasswordHash string
	BanUntil     *time.Time
	// TOTP-2FA fields (migration 008).
	TwoFactorEnabled bool
	TwoFactorSecret  []byte
	LastTOTPCounter  *int
	LastTOTPAt       *time.Time
}
