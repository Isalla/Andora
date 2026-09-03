package main

import (
	"time"

	_ "github.com/go-sql-driver/mysql"
)

// Account holds only the non-sensitive data needed to check a login.
// password_hash is used internally for the check and is never
// returned over the API.
type Account struct {
	ID           int
	Username     string
	PasswordHash string
	BanUntil     *time.Time
}
