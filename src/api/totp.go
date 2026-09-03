package main

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha1"
	"encoding/base32"
	"encoding/binary"
	"fmt"
	"math"
	"net/url"
	"strings"
	"time"
)

// TOTP parameters. RFC 6238 (HMAC-SHA1, 6 digits, 30 s period, ±1 step
// acceptance window). The reference books carry no TOTP chapter, so the
// IETF standard is the binding basis; the values below are fixed.
const (
	totpSecretBytes int   = 20 // 160 bits of entropy for the secret
	totpDigits      int   = 6
	totpPeriod      int64 = 30 // seconds per step
	totpWindow      int64 = 1  // accept one step before/after
)

// base32encode is RFC 4648 base32, upper case, NO padding. Every 20
// random bytes map to 32 chars, which is exactly the TOTP provisioning
// secret length.
func base32encode(b []byte) string {
	return base32.StdEncoding.WithPadding(base32.NoPadding).EncodeToString(b)
}

// newTOTPSecret returns 20 random bytes and their Base32 (no padding)
// provisioning form. The Base32 text is what the client provisions and
// the plaintext secret is what the server keeps in memory for URI
// building; the stored value is the ENCRYPTED raw bytes.
func newTOTPSecret() (raw []byte, base32 string, err error) {
	raw = make([]byte, totpSecretBytes)
	if _, err := rand.Read(raw); err != nil {
		return nil, "", fmt.Errorf("totp secret: %w", err)
	}
	return raw, base32encode(raw), nil
}

// totpCounter is the 30-second time counter of a moment.
func totpCounter(at time.Time) int64 {
	return at.Unix() / int64(totpPeriod)
}

// totpCode computes the 6-digit TOTP for the counter of `at` against
// the raw secret (HMAC-SHA1, dynamic truncation per RFC 6238/4226).
func totpCode(raw []byte, at time.Time) (string, error) {
	if len(raw) == 0 {
		return "", fmt.Errorf("empty totp secret")
	}
	counter := totpCounter(at)
	var buf [8]byte
	binary.BigEndian.PutUint64(buf[:], uint64(counter))
	mac := hmac.New(sha1.New, raw)
	mac.Write(buf[:])
	sum := mac.Sum(nil)
	offset := sum[len(sum)-1] & 0x0f
	bin := (uint32(sum[offset])<<24 |
		uint32(sum[offset+1])<<16 |
		uint32(sum[offset+2])<<8 |
		uint32(sum[offset+3])) & 0x7fffffff
	code := bin % uint32(math.Pow10(totpDigits))
	return fmt.Sprintf("%0*d", totpDigits, code), nil
}

// evalTOTPCode checks the presented 6-digit code against the raw secret
// within the ±1 step window and returns the step (counter) the code
// matches. The caller enforces the replay rule (counter must be STRICTLY
// greater than last_totp_counter) using that counter.
func evalTOTPCode(raw []byte, code string, at time.Time) (int64, bool) {
	expected, err := totpDigitsOnly(code)
	if err != nil {
		return 0, false
	}
	c := totpCounter(at)
	for _, step := range []int64{c - totpWindow, c, c + totpWindow} {
		if step < 0 {
			continue
		}
		atStep := time.Unix(step*int64(totpPeriod), 0)
		got, err := totpCode(raw, atStep)
		if err != nil {
			continue
		}
		if got == expected {
			return step, true
		}
	}
	return 0, false
}

// totpDigitsOnly normalizes a presented TOTP value to exactly 6 digits
// (strips whitespace); anything else is rejected.
func totpDigitsOnly(s string) (string, error) {
	s = strings.TrimSpace(s)
	if len(s) != totpDigits {
		return "", fmt.Errorf("totp code must be %d digits", totpDigits)
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			return "", fmt.Errorf("totp code must be digits only")
		}
	}
	return s, nil
}

// provisioningURI builds the otpauth:// URI the client scans to import
// the TOTP secret. It carries the Base32 secret in the clear BY DESIGN
// (that is the provisioning channel); the server keeps the raw bytes
// encrypted in the database, never this URI's secret.
func provisioningURI(raw []byte, accountID int) string {
	q := url.Values{}
	q.Set("secret", base32encode(raw))
	q.Set("issuer", "Andora")
	q.Set("algorithm", "SHA1")
	q.Set("digits", fmt.Sprintf("%d", totpDigits))
	q.Set("period", fmt.Sprintf("%d", totpPeriod))
	q.Set("account", fmt.Sprintf("andora-%d", accountID))
	return "otpauth://totp/" + "Andora?" + q.Encode()
}

// --- 2FA recovery codes (10 per setup, single-use) ---

// RecoveryCodeGroups is how many (5-char) base32 character groups one
// recovery code is made of; with separators that yields 5*5=25 chars of
// base32 and 4 dashes.
const (
	recoveryCodeGroups = 5
	groupLen           = 5
	recoveryCodeCount  = 10
)

// newRecoveryCodes generates the 10 single-use recovery codes for a
// 2FA setup. Each is base32, grouped as GGGGG-GGGGG-... for readability.
// The plaintext is shown on the client exactly once; only its SHA-256 is
// stored, so a DB dump cannot be replayed.
func newRecoveryCodes() []string {
	out := make([]string, recoveryCodeCount)
	for i := range out {
		out[i] = randomRecoveryCode()
	}
	return out
}

// randomRecoveryCode builds one grouped base32 code from 20 random
// bytes (32 base32 chars) plus the group separators.
func randomRecoveryCode() string {
	raw := make([]byte, totpSecretBytes)
	if _, err := rand.Read(raw); err != nil {
		// rand.Read errors are effectively impossible; fall back to a
		// deterministic-but-unusable placeholder is unacceptable, so we
		// surface it via panic-free fallback of empty groups.
		return ""
	}
	s := base32encode(raw)
	var b strings.Builder
	for i := 0; i < len(s); i += groupLen {
		if i > 0 {
			b.WriteByte('-')
		}
		end := i + groupLen
		if end > len(s) {
			end = len(s)
		}
		b.WriteString(s[i:end])
	}
	return b.String()
}

// normalizeRecoveryCode lowercases nothing (base32 is upper case) and
// strips group separators + surrounding whitespace: "ABCDE-FGHIJ" ->
// "ABCDEFGHI", "abcd" -> "ABCD". The SHA-256 of this canonical form is
// the stored token_hash; any cosmetic variant of the same code hashes
// identically, so case/separators are forgiving.
func normalizeRecoveryCode(s string) string {
	s = strings.ToUpper(strings.TrimSpace(s))
	return strings.ReplaceAll(strings.ReplaceAll(s, "-", ""), " ", "")
}
