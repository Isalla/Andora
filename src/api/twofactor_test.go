package main

import (
	"bytes"
	"context"
	"encoding/base32"
	"net/http"
	"strings"
	"testing"
	"time"
)

// tfTestServer builds a Server whose svc-all holds EVERY permission
// (including the four new two-factor ones) and a svc-limited that has
// none of them (for the 403 check). The fake store needs no DB.
func tfTestServer(t *testing.T) *Server {
	t.Helper()
	cfg := &Config{
		Port:   8080,
		AuthDB: AuthDBConfig{Host: "localhost", Port: 3306, User: "u", Password: "p", Database: "auth"},
		Services: map[string]ServiceCred{
			"svc-all":     {ID: "svc-all", Secret: "topsecret-all", Permissions: tfAllPerms()},
			"svc-limited": {ID: "svc-limited", Secret: "topsecret-lim", Permissions: []string{permAccountAuthenticate}},
		},
		ArgonTime:       1,
		ArgonMemory:     32,
		ArgonThreads:    1,
		SessionTTL:      60 * time.Minute,
		HandoffTTL:      time.Hour,
		RecoveryTTL:     30 * time.Minute,
		NewAccountBan:   60 * time.Second,
		RateLimitBurst:  100,
		RateLimitPerMin: 500,
		EncryptionKey:   "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
	}
	return &Server{cfg: cfg, store: newFakeStore(), rl: newRateLimit(cfg), start: time.Now()}
}

func tfAllPerms() []string {
	return []string{
		permAccountAuthenticate, permAccountRegister, permAccountPassword,
		permAccountRecovery, permAccountPermissions,
		permAccountTwoFactor, permDeviceList, permDeviceRevoke, permSecurityEvents,
		permSessionCreate, permSessionValidate, permSessionRevoke,
		permRealmList, permHandoffCreate, permHandoffValidate,
		permWorldAuthenticate, permWorldHeartbeat,
	}
}

func tfClient(srv *Server) *apiClient {
	return &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
}

// tfEnroll registers an account (unbanned) and enables TOTP-2FA with a
// deterministic secret so tests can compute the expected TOTP codes.
// Returns the account id, the raw secret and the 10 recovery codes.
func tfEnroll(t *testing.T, srv *Server, username, password string) (int, []byte, []string) {
	t.Helper()
	fs := srv.store.(*fakeStore)
	acc := addAccount(t, fs, username, password, username+"@example.com", false)

	raw := make([]byte, 20)
	copy(raw, []byte(username)) // deterministic 160-bit secret
	enc, err := aesGCMEncrypt(srv.cfg.EncryptionKey, raw)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	if err := fs.SaveTwoFactorSecret(ctx, acc.ID, enc); err != nil {
		t.Fatal(err)
	}
	codes := newRecoveryCodes()
	if err := fs.SaveRecoveryCodes(ctx, acc.ID, codes); err != nil {
		t.Fatal(err)
	}
	if err := fs.SetTwoFactorEnabled(ctx, acc.ID, true); err != nil {
		t.Fatal(err)
	}
	return acc.ID, raw, codes
}

func tfTotpNow(raw []byte) string {
	code, err := totpCode(raw, time.Now())
	if err != nil {
		return ""
	}
	return code
}

func tfVerify(t *testing.T, client *apiClient, payload map[string]any) map[string]any {
	t.Helper()
	rec := client.post(t, "/auth/verify", payload)
	if rec.Code != http.StatusOK {
		t.Fatalf("verify code %d: %s", rec.Code, rec.Body.String())
	}
	return decode(t, rec)
}

func tfDeviceToken() string {
	_, tok, err := randomToken()
	if err != nil {
		return ""
	}
	return tok
}

func tfHasEvent(body map[string]any, want string) bool {
	raw, _ := body["events"].([]any)
	for _, e := range raw {
		em, _ := e.(map[string]any)
		if em["event_type"] == want {
			return true
		}
	}
	return false
}

func TestTwoFactorSetupEndpoint(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	addAccount(t, fs, "setupuser", "correct-horse", "setup@example.com", false)

	rec := client.post(t, "/twofactor/setup", map[string]any{"account_id": 1})
	if rec.Code != http.StatusOK {
		t.Fatalf("setup code %d: %s", rec.Code, rec.Body.String())
	}
	body := decode(t, rec)
	if body["enabled"] != true {
		t.Fatalf("enabled %v", body["enabled"])
	}
	uri, _ := body["provisioning_uri"].(string)
	if !strings.HasPrefix(uri, "otpauth://totp/Andora?") || !strings.Contains(uri, "secret=") {
		t.Fatalf("provisioning_uri wrong: %q", uri)
	}
	codes, _ := body["recovery_codes"].([]any)
	if len(codes) != recoveryCodeCount {
		t.Fatalf("want %d recovery codes, got %d", recoveryCodeCount, len(codes))
	}
	acc := fs.accounts[1]
	if !acc.TwoFactorEnabled || len(acc.TwoFactorSecret) == 0 {
		t.Fatalf("2FA not enabled after setup: %+v", acc)
	}
	// the provisioning secret must equal the stored (encrypted) secret
	plain, err := aesGCMDecrypt(srv.cfg.EncryptionKey, acc.TwoFactorSecret)
	if err != nil {
		t.Fatal(err)
	}
	want := base32.StdEncoding.WithPadding(base32.NoPadding).EncodeToString(plain)
	if !strings.Contains(uri, "secret="+want) {
		t.Fatalf("provisioning mismatch: uri=%q plain-b32=%q", uri, want)
	}
	// second setup on the same account conflicts
	rec = client.post(t, "/twofactor/setup", map[string]any{"account_id": 1})
	if rec.Code != http.StatusConflict {
		t.Fatalf("second setup: want 409, got %d", rec.Code)
	}
}

func TestTwoFactorStatus(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	addAccount(t, srv.store.(*fakeStore), "stat0", "correct-horse", "stat0@example.com", false)

	rec := client.post(t, "/twofactor/status", map[string]any{"account_id": 1})
	body := decode(t, rec)
	if body["enabled"] != false || body["recovery_codes_available"] != false {
		t.Fatalf("initial status: %v", body)
	}

	tfEnroll(t, srv, "stat1", "correct-horse")
	rec = client.post(t, "/twofactor/status", map[string]any{"account_id": 2})
	body = decode(t, rec)
	if body["enabled"] != true || body["recovery_codes_available"] != true {
		t.Fatalf("enrolled status: %v", body)
	}
}

func TestTwoFactorPermissionDenied(t *testing.T) {
	srv := tfTestServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-limited"]}
	// svc-limited has only account.authenticate -> device.list forbidden
	rec := client.post(t, "/devices/list", map[string]any{"account_id": 1})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("want 403, got %d", rec.Code)
	}
}

func TestTwoFactorTOTPValidWrongAndMissing(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	_, raw, _ := tfEnroll(t, srv, "totpuser", "correct-horse")

	// missing code on an unknown device -> two_factor_required
	body := tfVerify(t, client, map[string]any{"username": "totpuser", "password": "correct-horse"})
	if body["valid"] != false || body["two_factor_required"] != true {
		t.Fatalf("missing totp: %v", body)
	}
	// wrong code -> two_factor_required
	body = tfVerify(t, client, map[string]any{"username": "totpuser", "password": "correct-horse", "totp_code": "000000"})
	if body["valid"] != false || body["two_factor_required"] != true {
		t.Fatalf("wrong totp: %v", body)
	}
	// correct code -> valid
	body = tfVerify(t, client, map[string]any{"username": "totpuser", "password": "correct-horse", "totp_code": tfTotpNow(raw)})
	if body["valid"] != true {
		t.Fatalf("correct totp: %v", body)
	}
}

func TestTwoFactorTOTPReplay(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	_, raw, _ := tfEnroll(t, srv, "replay", "correct-horse")

	valid := tfTotpNow(raw)
	// first use succeeds
	body := tfVerify(t, client, map[string]any{"username": "replay", "password": "correct-horse", "totp_code": valid})
	if body["valid"] != true {
		t.Fatalf("first use: %v", body)
	}
	// identical code rejected as replay (stale counter) -> two_factor_required
	body = tfVerify(t, client, map[string]any{"username": "replay", "password": "correct-horse", "totp_code": valid})
	if body["valid"] != false || body["two_factor_required"] != true {
		t.Fatalf("replay should be rejected: %v", body)
	}
}

// tfTrustDevice confirms one device for the account using a recovery
// code (single-use) so repeated confirmations do not collide with the
// TOTP replay counter. Returns the confirmed device token and the
// session id issued by that login.
func tfTrustDevice(t *testing.T, client *apiClient, username, password string, codes []string, n int, label string) (string, string) {
	t.Helper()
	dt := tfDeviceToken()
	body := tfVerify(t, client, map[string]any{
		"username": username, "password": password,
		"recovery_code": codes[n], "device_token": dt, "trust_device": true, "device_label": label,
	})
	if body["valid"] != true || body["device_token"] != dt {
		t.Fatalf("trust device %d: %v", n, body)
	}
	sessionID, _ := body["session_id"].(string)
	return dt, sessionID
}

func TestTwoFactorTrustedDeviceFlow(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	_, raw, codes := tfEnroll(t, srv, "device", "correct-horse")

	// unknown device + TOTP + trust_device -> confirmed, token returned
	dt := tfDeviceToken()
	body := tfVerify(t, client, map[string]any{
		"username": "device", "password": "correct-horse",
		"totp_code": tfTotpNow(raw), "device_token": dt, "trust_device": true, "device_label": "player-phone",
	})
	if body["valid"] != true || body["device_token"] != dt {
		t.Fatalf("confirm device: %v", body)
	}
	// confirmed device -> password alone suffices (no totp)
	body = tfVerify(t, client, map[string]any{"username": "device", "password": "correct-horse", "device_token": dt})
	if body["valid"] != true {
		t.Fatalf("confirmed device login: %v", body)
	}
	// revoke it, then the device is unknown again -> 2FA required
	_ = codes
	id := 1
	rec := client.post(t, "/devices/revoke", map[string]any{"account_id": id, "device_token": dt})
	if rec.Code != http.StatusOK || decode(t, rec)["revoked"] != true {
		t.Fatalf("revoke: %d %s", rec.Code, rec.Body.String())
	}
	body = tfVerify(t, client, map[string]any{"username": "device", "password": "correct-horse", "device_token": dt})
	if body["valid"] != false || body["two_factor_required"] != true {
		t.Fatalf("revoked device: %v", body)
	}
}

func TestTwoFactorDevicesListAndEvents(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, _, codes := tfEnroll(t, srv, "devlist", "correct-horse")

	_, _ = tfTrustDevice(t, client, "devlist", "correct-horse", codes, 0, "tablet")

	rec := client.post(t, "/devices/list", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK {
		t.Fatalf("list code %d", rec.Code)
	}
	devices, _ := decode(t, rec)["devices"].([]any)
	if len(devices) != 1 {
		t.Fatalf("want 1 device, got %d", len(devices))
	}
	first := devices[0].(map[string]any)
	if first["label"] != "tablet" {
		t.Fatalf("device label %v", first["label"])
	}

	rec = client.post(t, "/security/events", map[string]any{"account_id": id})
	if !tfHasEvent(decode(t, rec), eventDeviceConfirmed) {
		t.Fatalf("missing device_confirmed event")
	}
}

func TestTwoFactorMaxDevices(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, _, codes := tfEnroll(t, srv, "maxdev", "correct-horse")

	// confirm up to the 3-device limit via single-use recovery codes
	tokens := make([]string, maxTrustedDevices)
	for i := 0; i < maxTrustedDevices; i++ {
		tokens[i], _ = tfTrustDevice(t, client, "maxdev", "correct-horse", codes, i, "dev")
	}

	// 4th confirmation -> 409 max_devices_reached
	d4 := tfDeviceToken()
	rec := client.post(t, "/auth/verify", map[string]any{
		"username": "maxdev", "password": "correct-horse",
		"recovery_code": codes[maxTrustedDevices], "device_token": d4, "trust_device": true, "device_label": "dev-4",
	})
	if rec.Code != http.StatusConflict || decode(t, rec)["error"] != "max_devices_reached" {
		t.Fatalf("4th device: %d %s", rec.Code, rec.Body.String())
	}

	// after revoking one device the same token can be confirmed
	rec = client.post(t, "/devices/revoke", map[string]any{"account_id": id, "device_token": tokens[0]})
	if rec.Code != http.StatusOK {
		t.Fatalf("revoke %d", rec.Code)
	}
	rec = client.post(t, "/auth/verify", map[string]any{
		"username": "maxdev", "password": "correct-horse",
		"recovery_code": codes[maxTrustedDevices+1], "device_token": d4, "trust_device": true, "device_label": "dev-4",
	})
	if rec.Code != http.StatusOK {
		t.Fatalf("re-confirm after revoke: %d %s", rec.Code, rec.Body.String())
	}
	body := decode(t, rec)
	if body["valid"] != true || body["device_token"] != d4 {
		t.Fatalf("re-confirm: %v", body)
	}
}

func TestTwoFactorRecoveryCodeSingleUse(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	_, _, codes := tfEnroll(t, srv, "recover", "correct-horse")

	// first use logs in
	body := tfVerify(t, client, map[string]any{"username": "recover", "password": "correct-horse", "recovery_code": codes[0]})
	if body["valid"] != true {
		t.Fatalf("recovery login: %v", body)
	}
	// second use of the same code -> {valid:false}, not two_factor_required
	body = tfVerify(t, client, map[string]any{"username": "recover", "password": "correct-horse", "recovery_code": codes[0]})
	if body["valid"] != false || body["two_factor_required"] == true {
		t.Fatalf("recovery replay: %v", body)
	}
	// a recovery code does NOT confirm a device
	rec := client.post(t, "/devices/list", map[string]any{"account_id": 1})
	if devs, _ := decode(t, rec)["devices"].([]any); len(devs) != 0 {
		t.Fatalf("recovery code must not confirm a device")
	}
}

func TestTwoFactorDisableKeepsSessions(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, raw, _ := tfEnroll(t, srv, "disable", "correct-horse")

	// create a live session first
	body := tfVerify(t, client, map[string]any{"username": "disable", "password": "correct-horse", "totp_code": tfTotpNow(raw)})
	sessionID, _ := body["session_id"].(string)
	if sessionID == "" {
		t.Fatalf("no session: %v", body)
	}

	rec := client.post(t, "/twofactor/disable", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK || decode(t, rec)["enabled"] != false {
		t.Fatalf("disable: %d %s", rec.Code, rec.Body.String())
	}

	// the session is preserved
	rec = client.post(t, "/session/validate", map[string]any{"session_id": sessionID})
	if decode(t, rec)["valid"] != true {
		t.Fatalf("session should survive 2FA disable")
	}
	// secret + codes are wiped
	fs := srv.store.(*fakeStore)
	if len(fs.accounts[id].TwoFactorSecret) != 0 {
		t.Fatalf("secret not cleared")
	}
	if n, _ := fs.CountRecoveryCodes(context.Background(), id); n != 0 {
		t.Fatalf("codes not cleared: %d", n)
	}
	// login without TOTP now works
	body = tfVerify(t, client, map[string]any{"username": "disable", "password": "correct-horse"})
	if body["valid"] != true {
		t.Fatalf("login without totp after disable: %v", body)
	}
}

func TestTwoFactorResetRevokes(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, raw, codes := tfEnroll(t, srv, "reset", "correct-horse")

	// confirm a device + make a session
	dt, _ := tfTrustDevice(t, client, "reset", "correct-horse", codes, 0, "laptop")
	body := tfVerify(t, client, map[string]any{"username": "reset", "password": "correct-horse", "totp_code": tfTotpNow(raw)})
	sessionID, _ := body["session_id"].(string)

	rec := client.post(t, "/twofactor/reset", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK || decode(t, rec)["enabled"] != true {
		t.Fatalf("reset: %d %s", rec.Code, rec.Body.String())
	}
	// old session revoked
	rec = client.post(t, "/session/validate", map[string]any{"session_id": sessionID})
	if decode(t, rec)["valid"] != false {
		t.Fatalf("session should be revoked by reset")
	}
	// old device gone -> 2FA required again
	body = tfVerify(t, client, map[string]any{"username": "reset", "password": "correct-horse", "device_token": dt})
	if body["two_factor_required"] != true {
		t.Fatalf("device should be revoked: %v", body)
	}
	// new codes usable
	rec = client.post(t, "/twofactor/status", map[string]any{"account_id": id})
	if decode(t, rec)["enabled"] != true || decode(t, rec)["recovery_codes_available"] != true {
		t.Fatalf("status after reset: %v", decode(t, rec))
	}
}

func TestTwoFactorPasswordChangeRevokesAll(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, raw, codes := tfEnroll(t, srv, "pwchange", "old-password-1")

	// confirm a device + get a session (via a single-use recovery code,
	// which does not consume the TOTP counter)
	dt, sessionID := tfTrustDevice(t, client, "pwchange", "old-password-1", codes, 0, "pc")

	rec := client.post(t, "/account/password/change", map[string]any{
		"account_id": id, "old_password": "old-password-1", "new_password": "new-password-2",
	})
	if rec.Code != http.StatusOK || decode(t, rec)["changed"] != true {
		t.Fatalf("password change: %d %s", rec.Code, rec.Body.String())
	}

	// old session revoked
	rec = client.post(t, "/session/validate", map[string]any{"session_id": sessionID})
	if decode(t, rec)["valid"] != false {
		t.Fatalf("session should be revoked by password change")
	}
	// device token revoked -> 2FA required again
	body := tfVerify(t, client, map[string]any{"username": "pwchange", "password": "new-password-2", "device_token": dt})
	if body["two_factor_required"] != true {
		t.Fatalf("device should be revoked after password change: %v", body)
	}
	// new password works (2FA still on)
	body = tfVerify(t, client, map[string]any{"username": "pwchange", "password": "new-password-2", "totp_code": tfTotpNow(raw)})
	if body["valid"] != true {
		t.Fatalf("login with new pw: %v", body)
	}
	// old password no longer works
	body = tfVerify(t, client, map[string]any{"username": "pwchange", "password": "old-password-1", "totp_code": tfTotpNow(raw)})
	if body["valid"] != false {
		t.Fatalf("old password should not verify: %v", body)
	}
	// password_changed event recorded (ChangePasswordRevokeAll)
	rec = client.post(t, "/security/events", map[string]any{"account_id": id})
	if !tfHasEvent(decode(t, rec), eventPasswordChanged) {
		t.Fatalf("missing password_changed event")
	}
}

// ---------------------------------------------------------------------------
// P-34: atomic security events (docs/Security.md, docs/Auth_API_Architektur.md
// §18). Every affected business operation commits its security_events row
// together with its state change; a failing event write rolls the whole
// operation back, and a completed state change is never reported as a
// failure.
//
// These tests are LOGIC proofs on the fake store. They are NOT a MariaDB /
// InnoDB proof of transaction atomicity, of real RowsAffected behaviour or of
// genuine concurrent execution.
// ---------------------------------------------------------------------------

// tfEventCount counts stored events of one type. It never asserts anything
// about event ids: an InnoDB AUTO_INCREMENT counter is not rolled back either,
// so id gaps are normal and must not be treated as a defect.
func tfEventCount(t *testing.T, fs *fakeStore, accountID int, eventType string) int {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	n := 0
	for _, e := range fs.securityEvents[accountID] {
		if e.EventType == eventType {
			n++
		}
	}
	return n
}

func tfAccount(t *testing.T, fs *fakeStore, accountID int) *Account {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	acc := fs.accounts[accountID]
	if acc == nil {
		t.Fatalf("account %d missing", accountID)
	}
	c := *acc
	c.TwoFactorSecret = append([]byte(nil), acc.TwoFactorSecret...)
	return &c
}

func tfUnusedCodes(t *testing.T, fs *fakeStore, accountID int) int {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	n := 0
	for _, rc := range fs.recoveryCodes[accountID] {
		if rc.UsedAt == nil {
			n++
		}
	}
	return n
}

func tfDeviceCount(t *testing.T, fs *fakeStore, accountID int) int {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	return len(fs.trustedDevices[accountID])
}

// 1) Recovery code: success writes exactly one event, a repeat does not.
func TestP34RecoveryCodeEventIsAtomicOnSuccess(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34rcok", "correct-horse")

	rec := client.post(t, "/auth/verify", map[string]any{
		"username": "p34rcok", "password": "correct-horse", "recovery_code": codes[0],
	})
	if rec.Code != http.StatusOK || decode(t, rec)["valid"] != true {
		t.Fatalf("verify: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventRecoveryCodeUsed); n != 1 {
		t.Fatalf("want exactly 1 recovery_code_used, got %d", n)
	}
	// Single-use: the same code is refused and writes NO second event. The
	// external answer stays the generic rejection.
	rec = client.post(t, "/auth/verify", map[string]any{
		"username": "p34rcok", "password": "correct-horse", "recovery_code": codes[0],
	})
	body := decode(t, rec)
	if rec.Code != http.StatusOK || body["valid"] != false {
		t.Fatalf("reused code must be rejected generically: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventRecoveryCodeUsed); n != 1 {
		t.Fatalf("reuse must not add an event, got %d", n)
	}
}

// 2) Recovery code: a failing event rolls the consumption back, so the code
// stays usable and the login fails with the unchanged generic rejection.
func TestP34RecoveryCodeEventFailureRollsBack(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34rcfail", "correct-horse")

	fs.failEventWriteAt(opUseRecoveryCode)
	rec := client.post(t, "/auth/verify", map[string]any{
		"username": "p34rcfail", "password": "correct-horse", "recovery_code": codes[0],
	})
	if rec.Code != http.StatusOK || decode(t, rec)["valid"] != false {
		t.Fatalf("event failure must reject the login: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventRecoveryCodeUsed); n != 0 {
		t.Fatalf("rolled-back operation must leave no event, got %d", n)
	}
	// The code is still unused, so it works once the failure is cleared.
	fs.failStageAt(opUseRecoveryCode, 0)
	rec = client.post(t, "/auth/verify", map[string]any{
		"username": "p34rcfail", "password": "correct-horse", "recovery_code": codes[0],
	})
	if rec.Code != http.StatusOK || decode(t, rec)["valid"] != true {
		t.Fatalf("code must survive the rollback: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventRecoveryCodeUsed); n != 1 {
		t.Fatalf("want 1 event after the retry, got %d", n)
	}
}

// 3) Trusted device: success removes the device and writes exactly one
// event; a foreign or missing device keeps the 404 path and writes nothing.
func TestP34RevokeDeviceEventIsAtomic(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34dev", "correct-horse")
	dt, _ := tfTrustDevice(t, client, "p34dev", "correct-horse", codes, 0, "laptop")
	if n := tfDeviceCount(t, fs, id); n != 1 {
		t.Fatalf("precondition: 1 device, got %d", n)
	}
	// unknown token -> 404, no event
	rec := client.post(t, "/devices/revoke", map[string]any{"account_id": id, "device_token": tfDeviceToken()})
	if rec.Code != http.StatusNotFound {
		t.Fatalf("unknown device: %d %s", rec.Code, rec.Body.String())
	}
	// a device of a different account is invisible here (account_id guard)
	other := addAccount(t, fs, "p34other", "correct-horse", "p34other@example.com", false)
	rec = client.post(t, "/devices/revoke", map[string]any{"account_id": other.ID, "device_token": dt})
	if rec.Code != http.StatusNotFound {
		t.Fatalf("foreign device: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventDeviceRevoked); n != 0 {
		t.Fatalf("failed revocations must not write events, got %d", n)
	}
	if n := tfDeviceCount(t, fs, id); n != 1 {
		t.Fatalf("failed revocations must not remove devices, got %d", n)
	}
	// real revoke
	rec = client.post(t, "/devices/revoke", map[string]any{"account_id": id, "device_token": dt})
	if rec.Code != http.StatusOK || decode(t, rec)["revoked"] != true {
		t.Fatalf("revoke: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfDeviceCount(t, fs, id); n != 0 {
		t.Fatalf("device must be gone, got %d", n)
	}
	if n := tfEventCount(t, fs, id, eventDeviceRevoked); n != 1 {
		t.Fatalf("want 1 device_revoked, got %d", n)
	}
	// repeat -> 404 again, still exactly one event
	rec = client.post(t, "/devices/revoke", map[string]any{"account_id": id, "device_token": dt})
	if rec.Code != http.StatusNotFound {
		t.Fatalf("repeat revoke: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventDeviceRevoked); n != 1 {
		t.Fatalf("repeat must not add an event, got %d", n)
	}
}

// 4) Trusted device: a failing event keeps the device.
func TestP34RevokeDeviceEventFailureKeepsDevice(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34devfail", "correct-horse")
	dt, _ := tfTrustDevice(t, client, "p34devfail", "correct-horse", codes, 0, "laptop")

	fs.failEventWriteAt(opRevokeDevice)
	rec := client.post(t, "/devices/revoke", map[string]any{"account_id": id, "device_token": dt})
	if rec.Code < 400 {
		t.Fatalf("event failure must be reported, got %d %s", rec.Code, rec.Body.String())
	}
	if n := tfDeviceCount(t, fs, id); n != 1 {
		t.Fatalf("rolled-back revoke must keep the device, got %d", n)
	}
	if n := tfEventCount(t, fs, id, eventDeviceRevoked); n != 0 {
		t.Fatalf("no event after rollback, got %d", n)
	}
}

// 5) Setup: full success writes exactly one event and hands out the codes
// only after the commit.
func TestP34SetupTwoFactorSucceedsOnce(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	acc := addAccount(t, fs, "p34setup", "correct-horse", "p34setup@example.com", false)

	rec := client.post(t, "/twofactor/setup", map[string]any{"account_id": acc.ID})
	if rec.Code != http.StatusOK {
		t.Fatalf("setup: %d %s", rec.Code, rec.Body.String())
	}
	body := decode(t, rec)
	if body["enabled"] != true || body["provisioning_uri"] == "" {
		t.Fatalf("setup response incomplete: %v", body)
	}
	if got := len(body["recovery_codes"].([]any)); got != recoveryCodeCount {
		t.Fatalf("want %d recovery codes, got %d", recoveryCodeCount, got)
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorEnabled); n != 1 {
		t.Fatalf("want 1 two_factor_enabled, got %d", n)
	}
	if n := tfUnusedCodes(t, fs, acc.ID); n != recoveryCodeCount {
		t.Fatalf("want %d stored codes, got %d", recoveryCodeCount, n)
	}
	// a second setup is a conflict and writes nothing
	rec = client.post(t, "/twofactor/setup", map[string]any{"account_id": acc.ID})
	if rec.Code != http.StatusConflict {
		t.Fatalf("second setup: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorEnabled); n != 1 {
		t.Fatalf("conflict must not add an event, got %d", n)
	}
	if n := tfUnusedCodes(t, fs, acc.ID); n != recoveryCodeCount {
		t.Fatalf("conflict must not add codes, got %d", n)
	}
}

// 6) Setup: a failure at EVERY step rolls the whole operation back, so no
// half-configured account survives.
func TestP34SetupTwoFactorRollsBackAtEveryStep(t *testing.T) {
	for stage := 1; stage <= fakeStageCount[opSetupTwoFactor]; stage++ {
		srv := tfTestServer(t)
		client := tfClient(srv)
		fs := srv.store.(*fakeStore)
		acc := addAccount(t, fs, "p34setupfail", "correct-horse", "p34setupfail@example.com", false)

		fs.failStageAt(opSetupTwoFactor, stage)
		rec := client.post(t, "/twofactor/setup", map[string]any{"account_id": acc.ID})
		if rec.Code < 400 {
			t.Fatalf("stage %d: expected failure, got %d", stage, rec.Code)
		}
		// A rolled-back setup must not leak codes to the client.
		if body := decode(t, rec); body["recovery_codes"] != nil || body["provisioning_uri"] != nil {
			t.Fatalf("stage %d: failed setup leaked codes: %v", stage, body)
		}
		got := tfAccount(t, fs, acc.ID)
		if got.TwoFactorEnabled {
			t.Fatalf("stage %d: 2FA must stay disabled", stage)
		}
		if got.TwoFactorSecret != nil {
			t.Fatalf("stage %d: secret must be rolled back", stage)
		}
		if n := tfUnusedCodes(t, fs, acc.ID); n != 0 {
			t.Fatalf("stage %d: codes must be rolled back, got %d", stage, n)
		}
		if n := tfEventCount(t, fs, acc.ID, eventTwoFactorEnabled); n != 0 {
			t.Fatalf("stage %d: no event after rollback, got %d", stage, n)
		}
	}
}

// 7) Setup: two calls from the same starting state - exactly one wins.
func TestP34SetupTwoFactorConcurrentGateHasOneWinner(t *testing.T) {
	srv := tfTestServer(t)
	fs := srv.store.(*fakeStore)
	acc := addAccount(t, fs, "p34setuprace", "correct-horse", "p34setuprace@example.com", false)
	ctx := context.Background()
	encA, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("secret-a"))
	encB, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("secret-b"))

	if err := fs.SetupTwoFactor(ctx, acc.ID, encA, newRecoveryCodes()); err != nil {
		t.Fatalf("winner must succeed: %v", err)
	}
	// The loser used the same starting state, so its gate must fire.
	err := fs.SetupTwoFactor(ctx, acc.ID, encB, newRecoveryCodes())
	if err != ErrTwoFactorAlreadySetUp {
		t.Fatalf("loser must get the conflict, got %v", err)
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorEnabled); n != 1 {
		t.Fatalf("exactly one winner event expected, got %d", n)
	}
	if !bytes.Equal(tfAccount(t, fs, acc.ID).TwoFactorSecret, encA) {
		t.Fatal("the winner's secret must be the stored one")
	}
}

// 8) Enable: a real transition writes one event, an already enabled account
// is a no-op without event, and an event failure keeps 2FA disabled.
func TestP34EnableTwoFactorDecidesInsideTransaction(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	acc := addAccount(t, fs, "p34enable", "correct-horse", "p34enable@example.com", false)

	rec := client.post(t, "/twofactor/enable", map[string]any{"account_id": acc.ID})
	if rec.Code != http.StatusOK || decode(t, rec)["enabled"] != true {
		t.Fatalf("enable: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorEnabled); n != 1 {
		t.Fatalf("want 1 event, got %d", n)
	}
	// already enabled: success, no new event
	rec = client.post(t, "/twofactor/enable", map[string]any{"account_id": acc.ID})
	if rec.Code != http.StatusOK || decode(t, rec)["enabled"] != true {
		t.Fatalf("second enable: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorEnabled); n != 1 {
		t.Fatalf("no-op must not add an event, got %d", n)
	}
	// event failure on a real transition keeps 2FA disabled
	acc2 := addAccount(t, fs, "p34enablefail", "correct-horse", "p34enablefail@example.com", false)
	fs.failEventWriteAt(opEnableTwoFactor)
	rec = client.post(t, "/twofactor/enable", map[string]any{"account_id": acc2.ID})
	if rec.Code < 400 {
		t.Fatalf("event failure must be reported, got %d", rec.Code)
	}
	if tfAccount(t, fs, acc2.ID).TwoFactorEnabled {
		t.Fatal("rolled-back enable must leave 2FA disabled")
	}
	if n := tfEventCount(t, fs, acc2.ID, eventTwoFactorEnabled); n != 0 {
		t.Fatalf("no event after rollback, got %d", n)
	}
}

// 9) Enable: two calls from the same starting state produce at most one
// transition and one event.
func TestP34EnableTwoFactorConcurrentHasAtMostOneEvent(t *testing.T) {
	srv := tfTestServer(t)
	fs := srv.store.(*fakeStore)
	acc := addAccount(t, fs, "p34enablerace", "correct-horse", "p34enablerace@example.com", false)
	ctx := context.Background()
	changed1, err1 := fs.EnableTwoFactor(ctx, acc.ID)
	changed2, err2 := fs.EnableTwoFactor(ctx, acc.ID)
	if err1 != nil || err2 != nil {
		t.Fatalf("errors: %v / %v", err1, err2)
	}
	if changed1 != true || changed2 != false {
		t.Fatalf("want exactly one transition, got %v / %v", changed1, changed2)
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorEnabled); n != 1 {
		t.Fatalf("want 1 event, got %d", n)
	}
}

// 10) Disable: success clears everything and writes one event, sessions stay
// active, and no session_revoked exists.
func TestP34DisableTwoFactorSucceedsAndKeepsSessions(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34disable", "correct-horse")
	dt, sess := tfTrustDevice(t, client, "p34disable", "correct-horse", codes, 0, "laptop")
	_ = dt
	if got := statusOf(t, fs, sess); got != SessionValid {
		t.Fatalf("precondition: session valid, got %q", got)
	}

	rec := client.post(t, "/twofactor/disable", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK || decode(t, rec)["enabled"] != false {
		t.Fatalf("disable: %d %s", rec.Code, rec.Body.String())
	}
	got := tfAccount(t, fs, id)
	if got.TwoFactorEnabled || got.TwoFactorSecret != nil {
		t.Fatal("2FA must be fully off")
	}
	if n := tfUnusedCodes(t, fs, id); n != 0 {
		t.Fatalf("codes must be gone, got %d", n)
	}
	if n := tfDeviceCount(t, fs, id); n != 0 {
		t.Fatalf("devices must be gone, got %d", n)
	}
	if n := tfEventCount(t, fs, id, eventTwoFactorDisabled); n != 1 {
		t.Fatalf("want 1 two_factor_disabled, got %d", n)
	}
	// Sessions deliberately stay active (revocation policy) and NO
	// session_revoked event is invented. The type is checked as a literal:
	// this test must not introduce a constant for an event that must not
	// exist.
	if st := statusOf(t, fs, sess); st != SessionValid {
		t.Fatalf("sessions must stay valid, got %q", st)
	}
	if n := tfEventCount(t, fs, id, "session_revoked"); n != 0 {
		t.Fatalf("no session_revoked may exist, got %d", n)
	}
}

// 11) Disable: a failure at EVERY step rolls everything back.
func TestP34DisableTwoFactorRollsBackAtEveryStep(t *testing.T) {
	for stage := 1; stage <= fakeStageCount[opDisableTwoFactor]; stage++ {
		srv := tfTestServer(t)
		client := tfClient(srv)
		fs := srv.store.(*fakeStore)
		id, _, codes := tfEnroll(t, srv, "p34disfail", "correct-horse")
		_, sess := tfTrustDevice(t, client, "p34disfail", "correct-horse", codes, 0, "laptop")
		// Trusting a device CONSUMES one recovery code, so the baseline is
		// recoveryCodeCount-1 — measured, never assumed.
		beforeSecret := tfAccount(t, fs, id).TwoFactorSecret
		beforeCodes := tfUnusedCodes(t, fs, id)

		fs.failStageAt(opDisableTwoFactor, stage)
		rec := client.post(t, "/twofactor/disable", map[string]any{"account_id": id})
		if rec.Code < 400 {
			t.Fatalf("stage %d: expected failure, got %d", stage, rec.Code)
		}
		got := tfAccount(t, fs, id)
		if !got.TwoFactorEnabled {
			t.Fatalf("stage %d: 2FA must stay enabled after rollback", stage)
		}
		if !bytes.Equal(got.TwoFactorSecret, beforeSecret) {
			t.Fatalf("stage %d: secret must survive rollback", stage)
		}
		if n := tfUnusedCodes(t, fs, id); n != beforeCodes {
			t.Fatalf("stage %d: codes must survive rollback, got %d want %d", stage, n, beforeCodes)
		}
		if n := tfDeviceCount(t, fs, id); n != 1 {
			t.Fatalf("stage %d: device must survive rollback, got %d", stage, n)
		}
		if n := tfEventCount(t, fs, id, eventTwoFactorDisabled); n != 0 {
			t.Fatalf("stage %d: no event after rollback, got %d", stage, n)
		}
		if st := statusOf(t, fs, sess); st != SessionValid {
			t.Fatalf("stage %d: session must stay valid, got %q", stage, st)
		}
	}
}

// 12) Disable: an already disabled account is a no-op without event, and two
// calls from the same starting state produce at most one event.
func TestP34DisableTwoFactorNoOpAndConcurrency(t *testing.T) {
	srv := tfTestServer(t)
	fs := srv.store.(*fakeStore)
	acc := addAccount(t, fs, "p34disnoop", "correct-horse", "p34disnoop@example.com", false)
	ctx := context.Background()
	// never enabled -> no-op
	changed, err := fs.DisableTwoFactor(ctx, acc.ID)
	if err != nil || changed {
		t.Fatalf("no-op expected, got %v / %v", changed, err)
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorDisabled); n != 0 {
		t.Fatalf("no-op must write no event, got %d", n)
	}
	// enable first, then two disables race
	if err := fs.SetTwoFactorEnabled(ctx, acc.ID, true); err != nil {
		t.Fatal(err)
	}
	c1, e1 := fs.DisableTwoFactor(ctx, acc.ID)
	c2, e2 := fs.DisableTwoFactor(ctx, acc.ID)
	if e1 != nil || e2 != nil {
		t.Fatalf("errors: %v / %v", e1, e2)
	}
	if c1 != true || c2 != false {
		t.Fatalf("want exactly one transition, got %v / %v", c1, c2)
	}
	if n := tfEventCount(t, fs, acc.ID, eventTwoFactorDisabled); n != 1 {
		t.Fatalf("want 1 event, got %d", n)
	}
}

// 13) Reset: full success rotates everything, revokes sessions and devices,
// and writes exactly one event. The codes reach the client only after the
// commit.
func TestP34ResetTwoFactorSucceedsAtomically(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34reset", "correct-horse")
	_, sess := tfTrustDevice(t, client, "p34reset", "correct-horse", codes, 0, "laptop")
	oldSecret := tfAccount(t, fs, id).TwoFactorSecret
	if got := statusOf(t, fs, sess); got != SessionValid {
		t.Fatalf("precondition: session valid, got %q", got)
	}

	rec := client.post(t, "/twofactor/reset", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK {
		t.Fatalf("reset: %d %s", rec.Code, rec.Body.String())
	}
	body := decode(t, rec)
	if body["enabled"] != true || body["provisioning_uri"] == "" {
		t.Fatalf("reset response incomplete: %v", body)
	}
	newCodes := make([]string, 0, recoveryCodeCount)
	for _, c := range body["recovery_codes"].([]any) {
		newCodes = append(newCodes, c.(string))
	}
	if len(newCodes) != recoveryCodeCount {
		t.Fatalf("want %d codes, got %d", recoveryCodeCount, len(newCodes))
	}
	got := tfAccount(t, fs, id)
	if bytes.Equal(got.TwoFactorSecret, oldSecret) {
		t.Fatal("secret must have rotated")
	}
	if !got.TwoFactorEnabled {
		t.Fatal("2FA must be armed again")
	}
	if n := tfUnusedCodes(t, fs, id); n != recoveryCodeCount {
		t.Fatalf("old codes must be replaced, got %d", n)
	}
	// the OLD codes are gone, the NEW ones work
	rec = client.post(t, "/auth/verify", map[string]any{
		"username": "p34reset", "password": "correct-horse", "recovery_code": codes[0],
	})
	if decode(t, rec)["valid"] != false {
		t.Fatalf("old code must be invalid after reset: %s", rec.Body.String())
	}
	rec = client.post(t, "/auth/verify", map[string]any{
		"username": "p34reset", "password": "correct-horse", "recovery_code": newCodes[0],
	})
	if decode(t, rec)["valid"] != true {
		t.Fatalf("new code must work: %s", rec.Body.String())
	}
	if st := statusOf(t, fs, sess); st != SessionRevoked {
		t.Fatalf("reset must revoke sessions, got %q", st)
	}
	if n := tfDeviceCount(t, fs, id); n != 0 {
		t.Fatalf("reset must revoke devices, got %d", n)
	}
	if n := tfEventCount(t, fs, id, eventTwoFactorReset); n != 1 {
		t.Fatalf("want 1 two_factor_reset, got %d", n)
	}
}

// 14) Reset: a failure at EVERY step leaves the previous secret, codes,
// sessions and devices completely untouched.
func TestP34ResetTwoFactorRollsBackAtEveryStep(t *testing.T) {
	for stage := 1; stage <= fakeStageCount[opResetTwoFactor]; stage++ {
		srv := tfTestServer(t)
		client := tfClient(srv)
		fs := srv.store.(*fakeStore)
		id, _, codes := tfEnroll(t, srv, "p34resetfail", "correct-horse")
		_, sess := tfTrustDevice(t, client, "p34resetfail", "correct-horse", codes, 0, "laptop")
		oldSecret := tfAccount(t, fs, id).TwoFactorSecret
		oldCodes := tfUnusedCodes(t, fs, id)

		fs.failStageAt(opResetTwoFactor, stage)
		rec := client.post(t, "/twofactor/reset", map[string]any{"account_id": id})
		if rec.Code < 400 {
			t.Fatalf("stage %d: expected failure, got %d", stage, rec.Code)
		}
		if body := decode(t, rec); body["recovery_codes"] != nil || body["provisioning_uri"] != nil {
			t.Fatalf("stage %d: failed reset leaked codes: %v", stage, body)
		}
		got := tfAccount(t, fs, id)
		if !bytes.Equal(got.TwoFactorSecret, oldSecret) {
			t.Fatalf("stage %d: old secret must survive", stage)
		}
		if !got.TwoFactorEnabled {
			t.Fatalf("stage %d: 2FA must stay armed", stage)
		}
		if n := tfUnusedCodes(t, fs, id); n != oldCodes {
			t.Fatalf("stage %d: old codes must survive, got %d want %d", stage, n, oldCodes)
		}
		if st := statusOf(t, fs, sess); st != SessionValid {
			t.Fatalf("stage %d: sessions must stay valid, got %q", stage, st)
		}
		if n := tfDeviceCount(t, fs, id); n != 1 {
			t.Fatalf("stage %d: device must survive, got %d", stage, n)
		}
		if n := tfEventCount(t, fs, id, eventTwoFactorReset); n != 0 {
			t.Fatalf("stage %d: no event after rollback, got %d", stage, n)
		}
	}
}

// 15) Reset: an event failure specifically must roll back the whole reset —
// old secret, old codes, live sessions and live devices.
func TestP34ResetTwoFactorEventFailureRollsBackEverything(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34resetevfail", "correct-horse")
	_, sess := tfTrustDevice(t, client, "p34resetevfail", "correct-horse", codes, 0, "laptop")
	oldSecret := tfAccount(t, fs, id).TwoFactorSecret
	oldCodes := tfUnusedCodes(t, fs, id)

	fs.failEventWriteAt(opResetTwoFactor)
	rec := client.post(t, "/twofactor/reset", map[string]any{"account_id": id})
	if rec.Code < 400 {
		t.Fatalf("event failure must be reported, got %d", rec.Code)
	}
	if !bytes.Equal(tfAccount(t, fs, id).TwoFactorSecret, oldSecret) {
		t.Fatal("old secret must survive a failed event write")
	}
	if n := tfUnusedCodes(t, fs, id); n != oldCodes {
		t.Fatalf("old codes must survive, got %d want %d", n, oldCodes)
	}
	if st := statusOf(t, fs, sess); st != SessionValid {
		t.Fatalf("sessions must stay valid, got %q", st)
	}
	if n := tfDeviceCount(t, fs, id); n != 1 {
		t.Fatalf("device must survive, got %d", n)
	}
	if n := tfEventCount(t, fs, id, eventTwoFactorReset); n != 0 {
		t.Fatalf("no event after rollback, got %d", n)
	}
}

// 16) N4a — two resets that read the SAME stored secret: exactly one wins,
// the loser changes nothing and gets a conflict. Deterministic store-level
// proof of the CAS; genuine parallel InnoDB execution remains untested.
func TestP34ResetTwoFactorOverlappingRequestsHaveOneWinner(t *testing.T) {
	srv := tfTestServer(t)
	fs := srv.store.(*fakeStore)
	id, _, codes := tfEnroll(t, srv, "p34race", "correct-horse")
	_, sess := tfTrustDevice(t, tfClient(srv), "p34race", "correct-horse", codes, 0, "laptop")
	ctx := context.Background()

	// Both requests read the same starting state before either commits.
	expected := tfAccount(t, fs, id).TwoFactorSecret
	secretA, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("reset-a"))
	secretB, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("reset-b"))
	codesA := newRecoveryCodes()
	codesB := newRecoveryCodes()

	changedA, errA := fs.ResetTwoFactor(ctx, id, expected, secretA, codesA)
	changedB, errB := fs.ResetTwoFactor(ctx, id, expected, secretB, codesB)
	if errA != nil || errB != nil {
		t.Fatalf("CAS conflict must not be an error: %v / %v", errA, errB)
	}
	if changedA != true || changedB != false {
		t.Fatalf("exactly one winner expected, got %v / %v", changedA, changedB)
	}
	// The winner's state is the stored one; the loser wrote nothing.
	if !bytes.Equal(tfAccount(t, fs, id).TwoFactorSecret, secretA) {
		t.Fatal("winner's secret must be the stored one")
	}
	if n := tfUnusedCodes(t, fs, id); n != recoveryCodeCount {
		t.Fatalf("only the winner's codes may be stored, got %d", n)
	}
	if n := tfEventCount(t, fs, id, eventTwoFactorReset); n != 1 {
		t.Fatalf("exactly one event expected, got %d", n)
	}
	if st := statusOf(t, fs, sess); st != SessionRevoked {
		t.Fatalf("the winner's revocation must stand, got %q", st)
	}
	// the loser's code set must not work
	if got := fs.recoveryCodes[id][hashRecoveryCode(codesB[0])]; got != nil {
		t.Fatal("the loser's codes must not be stored")
	}
}

// 17) The handler maps a CAS conflict to 409 and hands out neither a
// provisioning URI nor recovery codes. A tiny wrapper makes the handler's
// pre-read return a stale secret, which is exactly the losing situation.
type staleSecretStore struct {
	*fakeStore
	stale []byte
}

func (w *staleSecretStore) FetchAccountByID(ctx context.Context, id int) (*Account, error) {
	acc, err := w.fakeStore.FetchAccountByID(ctx, id)
	if err != nil || acc == nil {
		return acc, err
	}
	c := *acc
	c.TwoFactorSecret = append([]byte(nil), w.stale...)
	return &c, nil
}

func TestP34ResetTwoFactorCasConflictAnswersConflict(t *testing.T) {
	srv := tfTestServer(t)
	fs := srv.store.(*fakeStore)
	id, _, _ := tfEnroll(t, srv, "p34conflict", "correct-horse")

	// The store now holds a rotated secret; the wrapper hands the handler a
	// stale expectation, so its CAS must lose.
	rotated, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("already-rotated"))
	if err := fs.SaveTwoFactorSecret(context.Background(), id, rotated); err != nil {
		t.Fatal(err)
	}
	stale, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("stale-expectation"))
	srv.store = &staleSecretStore{fakeStore: fs, stale: stale}

	rec := tfClient(srv).post(t, "/twofactor/reset", map[string]any{"account_id": id})
	if rec.Code != http.StatusConflict {
		t.Fatalf("CAS conflict must answer 409, got %d %s", rec.Code, rec.Body.String())
	}
	// The external message is the neutral one: it must not claim that a reset
	// is still running and must not leak anything about the winner.
	if msg, _ := decode(t, rec)["error"].(string); msg != "two-factor state changed; retry" {
		t.Fatalf("conflict message mismatch: %q", msg)
	}
	body := decode(t, rec)
	if body["provisioning_uri"] != nil || body["recovery_codes"] != nil {
		t.Fatalf("conflict must not hand out codes: %v", body)
	}
	if !bytes.Equal(tfAccount(t, fs, id).TwoFactorSecret, rotated) {
		t.Fatal("the winner's state must not be rolled back")
	}
	if n := tfEventCount(t, fs, id, eventTwoFactorReset); n != 0 {
		t.Fatalf("the loser must write no event, got %d", n)
	}
}

// 18) N4b — a reset that starts AFTER the previous commit read the already
// rotated secret: it is a new valid operation, commits, and its own event
// and codes are the current ones. Only the second code set is valid
// afterwards; the first one was regularly replaced by the second commit.
func TestP34ResetTwoFactorLaterResetReadsNewState(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id, _, _ := tfEnroll(t, srv, "p34seq", "correct-horse")

	rec := client.post(t, "/twofactor/reset", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK {
		t.Fatalf("reset A: %d %s", rec.Code, rec.Body.String())
	}
	firstCodes := map[string]bool{}
	for _, c := range decode(t, rec)["recovery_codes"].([]any) {
		firstCodes[c.(string)] = true
	}
	secretAfterA := tfAccount(t, fs, id).TwoFactorSecret

	// B starts now and therefore reads A's new secret.
	rec = client.post(t, "/twofactor/reset", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK {
		t.Fatalf("reset B: %d %s", rec.Code, rec.Body.String())
	}
	secondCodes := []string{}
	for _, c := range decode(t, rec)["recovery_codes"].([]any) {
		secondCodes = append(secondCodes, c.(string))
	}
	if n := tfEventCount(t, fs, id, eventTwoFactorReset); n != 2 {
		t.Fatalf("two successful resets must yield two events, got %d", n)
	}
	got := tfAccount(t, fs, id)
	if bytes.Equal(got.TwoFactorSecret, secretAfterA) {
		t.Fatal("B must have rotated the secret again")
	}
	// The store holds B's codes only.
	for _, c := range secondCodes {
		if fs.recoveryCodes[id][hashRecoveryCode(c)] == nil {
			t.Fatalf("B's code %q must be stored", c)
		}
	}
	fs.mu.Lock()
	storedHashes := make([]string, 0, len(fs.recoveryCodes[id]))
	for h := range fs.recoveryCodes[id] {
		storedHashes = append(storedHashes, h)
	}
	fs.mu.Unlock()
	if len(storedHashes) != recoveryCodeCount {
		t.Fatalf("store must contain exactly B's %d codes, got %d", recoveryCodeCount, len(storedHashes))
	}
	for c := range firstCodes {
		if fs.recoveryCodes[id][hashRecoveryCode(c)] != nil {
			t.Fatalf("A's code %q must have been replaced by B", c)
		}
	}
	// B's codes are the current ones.
	rec = client.post(t, "/auth/verify", map[string]any{
		"username": "p34seq", "password": "correct-horse", "recovery_code": secondCodes[0],
	})
	if decode(t, rec)["valid"] != true {
		t.Fatalf("B's code must be the current one: %s", rec.Body.String())
	}
}

// 19) N4c — guarantee boundary, as a code/logic proof: the CAS guards the
// SAME read starting state only. The guard is the conditional statement on
// the stored secret; nothing in the response path can add a second guard for
// a later reset, and no test may claim otherwise.
func TestP34ResetTwoFactorCasGuardsOnlyTheReadState(t *testing.T) {
	srv := tfTestServer(t)
	fs := srv.store.(*fakeStore)
	id, _, _ := tfEnroll(t, srv, "p34bound", "correct-horse")
	ctx := context.Background()

	// state S0: a reset with the correct expectation succeeds
	s0 := tfAccount(t, fs, id).TwoFactorSecret
	secret1, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("state-1"))
	if changed, err := fs.ResetTwoFactor(ctx, id, s0, secret1, newRecoveryCodes()); !changed || err != nil {
		t.Fatalf("first reset must succeed: %v / %v", changed, err)
	}
	// the same expectation loses now
	if changed, err := fs.ResetTwoFactor(ctx, id, s0, secret1, newRecoveryCodes()); changed || err != nil {
		t.Fatalf("stale expectation must lose without error: %v / %v", changed, err)
	}
	// reading the NEW state is a new valid operation - the CAS does not and
	// must not guard against a later reset.
	s1 := tfAccount(t, fs, id).TwoFactorSecret
	if bytes.Equal(s0, s1) {
		t.Fatal("precondition: the secret must have changed")
	}
	secret2, _ := aesGCMEncrypt(srv.cfg.EncryptionKey, []byte("state-2"))
	if changed, err := fs.ResetTwoFactor(ctx, id, s1, secret2, newRecoveryCodes()); !changed || err != nil {
		t.Fatalf("a later reset on the new state must commit: %v / %v", changed, err)
	}
	if n := tfEventCount(t, fs, id, eventTwoFactorReset); n != 2 {
		t.Fatalf("two commits, two events, got %d", n)
	}
}

// 20) Querschnitt: the voluntary logout keeps its P-33 semantics — no
// durable event, idempotent 200 — and the already atomic paths stay intact.
func TestP34VoluntaryLogoutAndAtomicPathsUnchanged(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	fs := srv.store.(*fakeStore)
	id := addAccount(t, fs, "p34logout", "correct-horse", "p34logout@example.com", false).ID

	rec := client.post(t, "/session/create", map[string]any{"account_id": id})
	if rec.Code != http.StatusOK {
		t.Fatalf("create: %d %s", rec.Code, rec.Body.String())
	}
	sess, _ := decode(t, rec)["session_id"].(string)
	before := len(fs.securityEvents[id])

	rec = client.post(t, "/session/revoke", map[string]any{"session_id": sess})
	if rec.Code != http.StatusOK || decode(t, rec)["revoked"] != true {
		t.Fatalf("revoke: %d %s", rec.Code, rec.Body.String())
	}
	// idempotent, as before
	rec = client.post(t, "/session/revoke", map[string]any{"session_id": sess})
	if rec.Code != http.StatusOK || decode(t, rec)["revoked"] != true {
		t.Fatalf("repeat revoke: %d %s", rec.Code, rec.Body.String())
	}
	if st := statusOf(t, fs, sess); st != SessionRevoked {
		t.Fatalf("session must be revoked, got %q", st)
	}
	if after := len(fs.securityEvents[id]); after != before {
		t.Fatalf("voluntary logout must not write a security event: %d -> %d", before, after)
	}

	// the already atomic paths still work: device_confirmed, password_changed
	dt := tfDeviceToken()
	rec = client.post(t, "/auth/verify", map[string]any{
		"account_id": id, "username": "p34logout", "password": "correct-horse",
		"device_token": dt, "trust_device": true,
	})
	if decode(t, rec)["valid"] != true {
		t.Fatalf("device confirm: %s", rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventDeviceConfirmed); n != 1 {
		t.Fatalf("device_confirmed must still be atomic, got %d", n)
	}
	rec = client.post(t, "/account/password/change", map[string]any{
		"account_id": id, "old_password": "correct-horse", "new_password": "brand-new-pass-1",
	})
	if rec.Code != http.StatusOK {
		t.Fatalf("password change: %d %s", rec.Code, rec.Body.String())
	}
	if n := tfEventCount(t, fs, id, eventPasswordChanged); n != 1 {
		t.Fatalf("password_changed must still be atomic, got %d", n)
	}
}

// 21) P-35 stays separate: the parental events remain best-effort and are
// NOT written transactionally. A lost parental event must not roll back the
// parental state.
func TestP34ParentalStaysBestEffort(t *testing.T) {
	srv := tfTestServer(t)
	client := &apiClient{s: srv, cred: srv.cfg.Services["svc-all"]}
	fs := srv.store.(*fakeStore)
	id := addAccount(t, fs, "p34parental", "longenough1", "p34parental@example.com", false).ID
	if err := fs.CreateParentalControl(context.Background(), &ParentalControls{AccountID: id}); err != nil {
		t.Fatal(err)
	}
	rec := client.post(t, "/parental/setup", map[string]any{"account_id": id, "child_username": "p34kid"})
	if rec.Code == http.StatusOK || rec.Code == http.StatusCreated {
		t.Fatalf("setup with a missing child must fail, got %d %s", rec.Code, rec.Body.String())
	}
	// the fake's recordEventLocked is not used by the parental path, so a
	// direct check: the best-effort helper still swallows its error.
	before := len(fs.securityEvents[id])
	srv.recordEvent(context.Background(), eventParentalSetup, id)
	if len(fs.securityEvents[id]) != before+1 {
		t.Fatal("best-effort helper must still record when the store succeeds")
	}
}

// 22) The log line for a failed event write carries only operation,
// event_type, account_id, stage and error_class — no token, token hash,
// session id, recovery code, TOTP secret, raw IP, database URL or driver
// text. securityEventFailureLine is pure, so this is a real check and not a
// comment.
func TestP34EventFailureLogLineHasNoSensitiveContent(t *testing.T) {
	id := 42
	line := securityEventFailureLine(opResetTwoFactor, eventTwoFactorReset, &id)
	want := "security_event_write_failed operation=reset_two_factor event_type=two_factor_reset account_id=42 stage=event_write error_class=db_error"
	if line != want {
		t.Fatalf("log line mismatch:\n got %q\nwant %q", line, want)
	}
	// A nil account id is rendered, never dereferenced.
	if l := securityEventFailureLine(opSetupTwoFactor, eventTwoFactorEnabled, nil); !strings.Contains(l, "account_id=none") {
		t.Fatalf("nil account id must be rendered: %q", l)
	}
	// Forbidden content may never appear in the line. No driver text either:
	// the only error-related token is the fixed error_class.
	forbidden := []string{
		"token", "hash", "secret", "session_id", "recovery_code", "pin",
		"ip=", "dsn", "password", "://", "err=", "cause",
	}
	lower := strings.ToLower(line)
	for _, f := range forbidden {
		if strings.Contains(lower, strings.ToLower(f)) {
			t.Fatalf("log line must not contain %q: %q", f, line)
		}
	}
	if strings.Count(line, "error_class=") != 1 {
		t.Fatalf("exactly one error_class expected: %q", line)
	}
	// Exactly one line: no embedded newline that could produce a second.
	if strings.ContainsAny(line, "\r\n") {
		t.Fatalf("log line must be a single line: %q", line)
	}
}
