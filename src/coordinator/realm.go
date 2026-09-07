package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

// realmResult is the terminal message the coordinator delivers to a
// realm (sections 11/12/13/18/24). It contains only coordinator-scope
// information; realms map result codes to their own local fallbacks.
type realmResult struct {
	RealmID   string `json:"realm_id"`
	JobID     string `json:"job_id"`
	JobType   string `json:"job_type"`
	Status    string `json:"status"` // COMPLETED or FAILED
	Result    string `json:"result,omitempty"`
	ErrorCode string `json:"error_code,omitempty"`
	ErrorMsg  string `json:"error_msg,omitempty"`
}

// realmClient delivers results to realms over the realm's configured
// result endpoint, signed with the coordinator's own service identity
// (mirror of src/api/auth.go authorize). Realms without a result URL
// poll GET /v1/jobs/{id} instead; this client simply has nothing to do
// for them.
type realmClient struct {
	svcID  string
	secret string
	http   *http.Client
}

func newRealmClient(cfg *Config) *realmClient {
	return &realmClient{
		svcID:  cfg.CoordinatorServiceID,
		secret: cfg.CoordinatorSecret,
		http:   &http.Client{Timeout: 10 * time.Second},
	}
}

// enabled reports whether results can be pushed at all (coordinator
// identity configured).
func (c *realmClient) enabled() bool {
	return c.svcID != "" && c.secret != ""
}

// deliver POSTs a terminal result to one realm. Failures are logged by
// the caller but never create new AI work (section 13); the realm can
// always query the job afterwards.
func (c *realmClient) deliver(ctx context.Context, cred RealmCred, res realmResult) error {
	if cred.ResultURL == "" || !c.enabled() {
		return nil
	}
	body, err := json.Marshal(res)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cred.ResultURL, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	ts := time.Now().Unix()
	req.Header.Set("X-Andora-Service", c.svcID)
	req.Header.Set("X-Andora-Timestamp", fmt.Sprintf("%d", ts))
	req.Header.Set("X-Andora-Signature", signPayload(c.secret, http.MethodPost, req.URL.Path, req.URL.RawQuery, ts, body))
	resp, err := c.http.Do(req)
	if err != nil {
		return fmt.Errorf("realm result callback: %w", err)
	}
	defer resp.Body.Close()
	io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("realm result callback status %d", resp.StatusCode)
	}
	return nil
}
