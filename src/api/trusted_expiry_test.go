package main

import (
	"context"
	"database/sql"
	"errors"
	"net/http"
	"testing"
	"time"
)

// TestTrustedDeviceActiveBoundary pins the pure expiry helper at its
// boundary: exactly 30 days is still active, one nanosecond beyond is
// expired.
func TestTrustedDeviceActiveBoundary(t *testing.T) {
	now := time.Date(2026, 9, 4, 12, 0, 0, 0, time.UTC)
	if !trustedDeviceActive(now.Add(-30*24*time.Hour), now) {
		t.Fatal("exactly 30 days must still be active")
	}
	if trustedDeviceActive(now.Add(-30*24*time.Hour).Add(-time.Nanosecond), now) {
		t.Fatal("one nanosecond beyond 30 days must be expired")
	}
	if !trustedDeviceActive(now.Add(-time.Minute), now) {
		t.Fatal("recent usage must be active")
	}
}

// tfExpireDeviceBackdates a confirmed device's last_used_at beyond the
// 30-day window so it reads as expired.
func tfExpireDeviceBackdate(t *testing.T, fs *fakeStore, accountID int, rawToken string) {
	t.Helper()
	fs.mu.Lock()
	defer fs.mu.Unlock()
	d, ok := fs.trustedDevices[accountID][tokenHash(rawToken)]
	if !ok {
		t.Fatalf("device %q not confirmed for account %d", rawToken, accountID)
	}
	d.LastUsedAt = time.Now().Add(-31 * 24 * time.Hour)
	fs.trustedDevices[accountID][tokenHash(rawToken)] = d
}

// TestTrustedDeviceExpiryStore covers the store-level contract: fresh
// confirmed devices are active, backdated ones are not, touching
// re-activates, and re-presenting an expired token must not collide
// (the expired row is purged within the same operation).
func TestTrustedDeviceExpiryStore(t *testing.T) {
	ctx := context.Background()
	fs := newFakeStore()
	id := addAccount(t, fs, "expuser", "correct-horse", "e@example.com", false).ID

	dt := tfDeviceToken()
	d, err := fs.AddTrustedDevice(ctx, id, dt, "phone")
	if err != nil {
		t.Fatalf("add: %v", err)
	}
	if d.LastUsedAt.IsZero() {
		t.Fatal("LastUsedAt must be set on confirm")
	}
	ok, err := fs.HasTrustedDevice(ctx, id, dt)
	if err != nil || !ok {
		t.Fatalf("fresh device must be active: ok=%v err=%v", ok, err)
	}
	if n, err := fs.CountTrustedDevices(ctx, id); err != nil || n != 1 {
		t.Fatalf("count: n=%d err=%v", n, err)
	}

	tfExpireDeviceBackdate(t, fs, id, dt)

	ok, err = fs.HasTrustedDevice(ctx, id, dt)
	if err != nil || ok {
		t.Fatalf("expired device must NOT be active: ok=%v err=%v", ok, err)
	}
	if n, _ := fs.CountTrustedDevices(ctx, id); n != 0 {
		t.Fatalf("expired device must not count against the limit: %d", n)
	}

	if err := fs.TouchTrustedDevice(ctx, id, dt); err != nil {
		t.Fatalf("touch: %v", err)
	}
	ok, err = fs.HasTrustedDevice(ctx, id, dt)
	if err != nil || !ok {
		t.Fatalf("touched device must be active: ok=%v err=%v", ok, err)
	}
	fs.mu.Lock()
	touched, _ := fs.trustedDevices[id][tokenHash(dt)]
	fs.mu.Unlock()
	if touched.LastUsedAt.IsZero() {
		t.Fatal("LastUsedAt not refreshed by touch")
	}

	if err := fs.TouchTrustedDevice(ctx, id, tfDeviceToken()); !errors.Is(err, sql.ErrNoRows) {
		t.Fatalf("touch unknown device: want ErrNoRows, got %v", err)
	}

	// A token used again after expiry must not collide with the unique
	// token_hash key of its expired row.
	tfExpireDeviceBackdate(t, fs, id, dt)
	if _, err := fs.AddTrustedDevice(ctx, id, dt, "phone-again"); err != nil {
		t.Fatalf("re-confirm after expiry: %v", err)
	}
	if n, _ := fs.CountTrustedDevices(ctx, id); n != 1 {
		t.Fatalf("re-confirmed device must count: %d", n)
	}
}

// TestTrustedDeviceRefreshLastUsed checks that a successful login with
// a confirmed device refreshes its last_used_at inside the handler
// (best-effort: the login itself still succeeds).
func TestTrustedDeviceRefreshLastUsed(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, _, codes := tfEnroll(t, srv, "refuser", "correct-horse")
	fs := srv.store.(*fakeStore)
	dt, _ := tfTrustDevice(t, client, "refuser", "correct-horse", codes, 0, "phone")

	fs.mu.Lock()
	before, _ := fs.trustedDevices[id][tokenHash(dt)]
	before.LastUsedAt = time.Now().Add(-2 * time.Hour) // rewind, still within 30 days
	fs.trustedDevices[id][tokenHash(dt)] = before
	fs.mu.Unlock()
	oldTS := before.LastUsedAt

	body := tfVerify(t, client, map[string]any{
		"username": "refuser", "password": "correct-horse", "device_token": dt,
	})
	if body["valid"] != true {
		t.Fatalf("login with confirmed device: %v", body)
	}

	fs.mu.Lock()
	after, _ := fs.trustedDevices[id][tokenHash(dt)]
	fs.mu.Unlock()
	if !after.LastUsedAt.After(oldTS) {
		t.Fatalf("last_used_at not refreshed on login: before=%v after=%v", oldTS, after.LastUsedAt)
	}
}

// TestTrustedDeviceExpiredBlocksDeviceLogin proves the endpoint effect:
// a confirmed device that is 31 days stale no longer bypasses 2FA, but
// the account itself can still log in via TOTP (no account-level
// expiry exists).
func TestTrustedDeviceExpiredBlocksDeviceLogin(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, raw, codes := tfEnroll(t, srv, "explog", "correct-horse")
	fs := srv.store.(*fakeStore)
	dt, _ := tfTrustDevice(t, client, "explog", "correct-horse", codes, 0, "phone")

	tfExpireDeviceBackdate(t, fs, id, dt)

	body := tfVerify(t, client, map[string]any{
		"username": "explog", "password": "correct-horse", "device_token": dt,
	})
	if body["valid"] != false || body["two_factor_required"] != true {
		t.Fatalf("expired device must not bypass 2FA: %v", body)
	}

	body = tfVerify(t, client, map[string]any{
		"username": "explog", "password": "correct-horse", "totp_code": tfTotpNow(raw),
	})
	if body["valid"] != true {
		t.Fatalf("account itself must still verify via TOTP: %v", body)
	}
}

// TestTrustedDeviceExpiryFreesSlot proves the 3-device limit only
// counts active devices: an expired device frees its slot, so a 4th
// confirmation is possible again.
func TestTrustedDeviceExpiryFreesSlot(t *testing.T) {
	srv := tfTestServer(t)
	client := tfClient(srv)
	id, _, codes := tfEnroll(t, srv, "slotuser", "correct-horse")
	fs := srv.store.(*fakeStore)

	tokens := make([]string, maxTrustedDevices)
	for i := 0; i < maxTrustedDevices; i++ {
		tokens[i], _ = tfTrustDevice(t, client, "slotuser", "correct-horse", codes, i, "dev")
	}

	d4 := tfDeviceToken()
	rec := client.post(t, "/auth/verify", map[string]any{
		"username": "slotuser", "password": "correct-horse",
		"recovery_code": codes[3], "device_token": d4, "trust_device": true, "device_label": "dev-4",
	})
	if rec.Code != http.StatusConflict || decode(t, rec)["error"] != "max_devices_reached" {
		t.Fatalf("4th device: %d %s", rec.Code, rec.Body.String())
	}

	tfExpireDeviceBackdate(t, fs, id, tokens[0])

	rec = client.post(t, "/auth/verify", map[string]any{
		"username": "slotuser", "password": "correct-horse",
		"recovery_code": codes[4], "device_token": d4, "trust_device": true, "device_label": "dev-4",
	})
	if rec.Code != http.StatusOK || decode(t, rec)["valid"] != true {
		t.Fatalf("4th device after one expiry: %d %s", rec.Code, rec.Body.String())
	}
}
