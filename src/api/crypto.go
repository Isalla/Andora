package main

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// randomToken returns 32 random bytes as a 64-char hex string. Random
// tokens are never stored in the clear; only their SHA-256 hash lives
// in the database.
func randomToken() (time.Time, string, error) {
	buf := make([]byte, 32)
	if _, err := rand.Read(buf); err != nil {
		return time.Time{}, "", err
	}
	return time.Now(), hex.EncodeToString(buf), nil
}

// hashLookupEmail is the non-reversible E-Mail-Lookup-Hash: SHA-256 of
// the normalized e-mail address, as 64 hex chars (the form the API
// accepts in requests) — the hex of the same digest as hashLookupEmailRaw.
func hashLookupEmail(email string) string {
	return hex.EncodeToString(hashLookupEmailRaw(email))
}

// hashLookupEmailRaw is the same digest as raw 32 bytes — the form the
// auth database stores in email_lookup_hash (BINARY(32)).
func hashLookupEmailRaw(email string) []byte {
	sum := sha256.Sum256([]byte(normalizeEmail(email)))
	return sum[:]
}

func normalizeEmail(email string) string {
	return strings.ToLower(strings.TrimSpace(email))
}

// --- AES-256-GCM core ---
//
// The key is a 32-byte, hex-encoded value from the service config
// (ENCRYPTION_KEY). It is stored inside config.env (outside git), never
// in the DB and never returned by the API. Ciphertext is
// nonce || tag || ciphertext with a freshly generated nonce per record.
// The same core encrypts e-mails AND the TOTP secret.
func aesGCMEncrypt(keyHex string, plain []byte) ([]byte, error) {
	key, err := hex.DecodeString(keyHex)
	if err != nil || len(key) != 32 {
		return nil, fmt.Errorf("bad encryption key")
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("init cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("init gcm: %w", err)
	}
	nonce := make([]byte, gcm.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, fmt.Errorf("nonce: %w", err)
	}
	ct := gcm.Seal(nil, nonce, plain, nil)
	out := make([]byte, 12+len(ct))
	copy(out, nonce)
	copy(out[12:], ct)
	return out, nil
}

// aesGCMDecrypt decrypts nonce || tag || ciphertext produced by
// aesGCMEncrypt.
func aesGCMDecrypt(keyHex string, enc []byte) ([]byte, error) {
	key, err := hex.DecodeString(keyHex)
	if err != nil || len(key) != 32 {
		return nil, fmt.Errorf("bad encryption key")
	}
	if len(enc) < 12+16 {
		return nil, fmt.Errorf("ciphertext too short")
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("init cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("init gcm: %w", err)
	}
	nonce, rest := enc[:12], enc[12:]
	plain, err := gcm.Open(nil, nonce, rest, nil)
	if err != nil {
		return nil, fmt.Errorf("decrypt: %w", err)
	}
	return plain, nil
}

// --- e-mail encryption (base64url of the AES-GCM core) ---

func decryptEmail(keyHex, b64 string) (string, error) {
	enc, err := base64.RawURLEncoding.DecodeString(b64)
	if err != nil {
		return "", fmt.Errorf("bad e-mail ciphertext")
	}
	plain, err := aesGCMDecrypt(keyHex, enc)
	if err != nil {
		return "", err
	}
	return string(plain), nil
}

func encryptEmail(keyHex, email string) (string, error) {
	enc, err := aesGCMEncrypt(keyHex, []byte(email))
	if err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(enc), nil
}

// hmacEqual is a constant-time string compare.
func hmacEqual(a, b string) bool {
	return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1
}
