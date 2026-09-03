package main

import (
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
