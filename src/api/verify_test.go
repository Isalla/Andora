package main

import (
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"
)

func TestArgonVerifyRoundtrip(t *testing.T) {
	h := HashPassword("s3cret-PW", "0123456789abcd", 1, 19456, 4)
	if !VerifyPassword(h, "s3cret-PW") {
		t.Fatal("correct password should verify")
	}
	if VerifyPassword(h, "wrong") {
		t.Fatal("wrong password should not verify")
	}
	if VerifyPassword("garbage", "s3cret-PW") {
		t.Fatal("bad format must not verify")
	}
}

// verifyPassword must be self-describing: it must not depend on the
// caller passing parameters. Check that a hash created with one set of
// params still verifies regardless of what the current config would be.
func TestArgonVerifyIndependentOfConfig(t *testing.T) {
	// created with time=1 mem=19456 threads=4
	h := HashPassword("pw", "0123456789abcd", 1, 19456, 4)
	// even a caller using totally different params must still verify:
	if !VerifyPassword(h, "pw") {
		t.Fatal("verify must succeed with params embedded in hash")
	}
}

func TestConfigParse(t *testing.T) {
	dir := t.TempDir()
	p := filepath.Join(dir, "env.test")
	body := `
# comment
AUTHAPI_PORT=9000
AUTH_DB_HOST=db.local
AUTH_DB_PORT=3306
AUTH_DB_USER=andora_auth
AUTH_DB_PASSWORD=devpass
SERVICE_login_ID=login-service
SERVICE_login_SECRET=topsecret1
SERVICE_login_PERMISSIONS=account.authenticate,
SERVICE_web_ID=web-service
SERVICE_web_SECRET=topsecret2
SERVICE_web_PERMISSIONS=account.authenticate
`
	os.WriteFile(p, []byte(body), 0600)
	cfg, err := loadConfig(p)
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Port != 9000 {
		t.Fatalf("port %d", cfg.Port)
	}
	if len(cfg.Services) != 2 {
		t.Fatalf("services %v", cfg.Services)
	}
	login := cfg.Services["login-service"]
	if login.ID != "login-service" || login.Secret != "topsecret1" ||
		len(login.Permissions) != 1 || login.Permissions[0] != "account.authenticate" {
		t.Fatalf("login cred: %v", login)
	}
}

func TestSignPayloadDeterministic(t *testing.T) {
	a := signPayload("secret", "POST", "/auth/verify", "", 111, []byte("x"))
	b := signPayload("secret", "POST", "/auth/verify", "", 111, []byte("x"))
	if a != b {
		t.Fatal("signature must be deterministic")
	}
	c := signPayload("secret", "POST", "/auth/verify", "", 112, []byte("x"))
	if a == c {
		t.Fatal("different ts must change signature")
	}
	_ = hex.EncodeToString([]byte{})
}
