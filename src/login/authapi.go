package main

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

// AuthAPI is a signed client for the Auth/API-Service. Signature scheme,
// headers and timestamp window mirror src/api/auth.go (authorize):
//
//	X-Andora-Service:   <service id>
//	X-Andora-Timestamp: unix seconds
//	X-Andora-Signature: hex(HMAC-SHA256(secret, payload))
//
// with payload = METHOD \n PATH \n RAW_QUERY \n TIMESTAMP \n SHA256(BODY).
type AuthAPI struct {
	base   string
	svcID  string
	secret string
	client *http.Client
}

func newAuthAPI(cfg *Config) *AuthAPI {
	return &AuthAPI{
		base:   cfg.AuthAPIURL,
		svcID:  cfg.ServiceID,
		secret: cfg.Secret,
		client: &http.Client{Timeout: cfg.AuthAPITimeout},
	}
}

func sha256Hex(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func signPayload(secret, method, path, rawQuery string, ts int64, body []byte) string {
	payload := fmt.Sprintf("%s\n%s\n%s\n%d\n%s",
		method, path, rawQuery, ts, sha256Hex(body))
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write([]byte(payload))
	return hex.EncodeToString(mac.Sum(nil))
}

// AuthAPIError reports a transport-level or Auth-API-side failure.
// Fail-closed: callers map this to 503 without leaking details.
type AuthAPIError struct {
	Status int
	Body   string
}

func (e *AuthAPIError) Error() string {
	if e.Status != 0 {
		return fmt.Sprintf("auth api status %d", e.Status)
	}
	return "auth api unreachable"
}

// post sends a signed POST with a JSON body and decodes the JSON
// answer into out. Non-2xx answers become *AuthAPIError.
func (a *AuthAPI) post(path string, in any, out any) error {
	var body []byte
	if in == nil {
		body = []byte("{}")
	} else {
		var err error
		body, err = json.Marshal(in)
		if err != nil {
			return fmt.Errorf("encode request: %w", err)
		}
	}
	ts := time.Now().Unix()
	req, err := http.NewRequest(http.MethodPost, a.base+path, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Andora-Service", a.svcID)
	req.Header.Set("X-Andora-Timestamp", fmt.Sprintf("%d", ts))
	req.Header.Set("X-Andora-Signature", signPayload(a.secret, http.MethodPost, path, "", ts, body))
	resp, err := a.client.Do(req)
	if err != nil {
		return &AuthAPIError{}
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 64*1024))
	if err != nil {
		return &AuthAPIError{Status: resp.StatusCode}
	}
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return &AuthAPIError{Status: resp.StatusCode, Body: string(raw)}
	}
	if out != nil {
		if err := json.Unmarshal(raw, out); err != nil {
			return fmt.Errorf("decode auth answer: %w", err)
		}
	}
	return nil
}

// --- answer shapes (subset of the Auth-API contract) ---

// VerifyAnswer mirrors the /auth/verify response. Failure shapes stay
// distinct-by-design (see src/api/handlers.go).
type VerifyAnswer struct {
	Valid             bool   `json:"valid"`
	AccountID         int    `json:"account_id,omitempty"`
	SessionID         string `json:"session_id,omitempty"`
	ExpiresAt         string `json:"expires_at,omitempty"`
	DeviceToken       string `json:"device_token,omitempty"`
	TwoFactorRequired bool   `json:"two_factor_required,omitempty"`
	ParentalBlocked   bool   `json:"parental_blocked,omitempty"`
}

// SessionAnswer mirrors the /session/validate response.
type SessionAnswer struct {
	Valid     bool   `json:"valid"`
	AccountID int    `json:"account_id,omitempty"`
	ExpiresAt string `json:"expires_at,omitempty"`
}

// Realm mirrors one entry of the /realms response (auth DB row).
type Realm struct {
	ID              int     `json:"id"`
	Name            string  `json:"name"`
	Language        string  `json:"language"`
	Region          string  `json:"region"`
	Enabled         bool    `json:"enabled"`
	FreshStartUntil *string `json:"fresh_start_until"`
	TransferPolicy  string  `json:"transfer_policy"`
}

// HandoffAnswer mirrors the /handoff/create response.
type HandoffAnswer struct {
	HandoffToken string `json:"handoff_token"`
	ExpiresAt    string `json:"expires_at"`
}

// Verify forwards a login check (password, 2FA fields) to /auth/verify.
func (a *AuthAPI) Verify(in map[string]any) (*VerifyAnswer, error) {
	var out VerifyAnswer
	if err := a.post("/auth/verify", in, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ValidateSession checks a login session, returning account binding.
func (a *AuthAPI) ValidateSession(sessionID string) (*SessionAnswer, error) {
	var out SessionAnswer
	if err := a.post("/session/validate", map[string]any{"session_id": sessionID}, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ListRealms returns the registered realms (business data only).
func (a *AuthAPI) ListRealms() ([]Realm, error) {
	var out struct {
		Realms []Realm `json:"realms"`
	}
	if err := a.post("/realms", nil, &out); err != nil {
		return nil, err
	}
	if out.Realms == nil {
		out.Realms = []Realm{}
	}
	return out.Realms, nil
}

// CreateHandoff binds an account to a realm (single-use token).
func (a *AuthAPI) CreateHandoff(accountID, realmID int) (*HandoffAnswer, error) {
	var out HandoffAnswer
	if err := a.post("/handoff/create",
		map[string]any{"account_id": accountID, "realm_id": realmID}, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// RevokeSession deletes a login session (logout).
func (a *AuthAPI) RevokeSession(sessionID string) error {
	var out map[string]any
	return a.post("/session/revoke", map[string]any{"session_id": sessionID}, &out)
}
