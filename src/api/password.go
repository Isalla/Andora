package main

import (
	"crypto/subtle"
	"encoding/base64"
	"fmt"
	"strconv"
	"strings"

	"golang.org/x/crypto/argon2"
)

// Stored password format used in accounts.password_hash:
//
//	$argon2$<time>$<memoryKiB>$<threads>$<salt>$<hash>
//
// <salt> and <hash> are base64url. Time / memory / threads are part
// of the hash so verification is self-describing; changing those
// parameters later is a new migration, not an edit of an applied one.
const passwordHashPrefix = "$argon2$"

// argonIDKey derives a 32-byte key with Argon2id.
func argonIDKey(salt, password string, time_, memory uint32, threads uint8) []byte {
	return argon2.IDKey([]byte(salt), []byte(password), time_, memory, threads, 32)
}

// HashPassword produces the stored hash for (password, salt).
// Exposed so later migration / re-hash tooling can create hashes with
// the parameters currently in force.
func HashPassword(password, salt string, time_, memory uint32, threads uint8) string {
	dk := argonIDKey(salt, password, time_, memory, threads)
	return fmt.Sprintf("%s%d$%d$%d$%s$%s",
		passwordHashPrefix, time_, memory, threads,
		base64.RawURLEncoding.EncodeToString([]byte(salt)),
		base64.RawURLEncoding.EncodeToString(dk))
}

// VerifyPassword checks a candidate password against a stored hash.
// Parameters (time/memory/threads) are taken from the hash itself, so
// verification is independent of current config. Unrecognised formats
// return false without error. Constant-time compare avoids timing leaks.
func VerifyPassword(stored, candidate string) bool {
	if !strings.HasPrefix(stored, passwordHashPrefix) {
		return false
	}
	parts := strings.Split(stored, "$")
	// ["", "argon2", time, memory, threads, salt, hash]
	if len(parts) != 7 || parts[1] != "argon2" {
		return false
	}
	t, err := strconv.ParseUint(parts[2], 10, 32)
	if err != nil {
		return false
	}
	m, err := strconv.ParseUint(parts[3], 10, 32)
	if err != nil {
		return false
	}
	th, err := strconv.ParseUint(parts[4], 10, 8)
	if err != nil {
		return false
	}
	salt, err := base64.RawURLEncoding.DecodeString(parts[5])
	if err != nil {
		return false
	}
	want, err := base64.RawURLEncoding.DecodeString(parts[6])
	if err != nil || len(want) != 32 {
		return false
	}
	got := argonIDKey(string(salt), candidate, uint32(t), uint32(m), uint8(th))
	return subtle.ConstantTimeCompare(got, want) == 1
}
