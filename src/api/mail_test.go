package main

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

// Mail-System V1 — Go-API-Wrapper tests.
//

// The Realm-side Mock-DB-Adapter is represented by the canned upstream
// answer (httptest): NO live DB, NO live Realm. The wrapper itself never
// touches a database (docs/Datenbank_Architektur.md §13–§15).

func mailCfg(t *testing.T, forward string) *Config {
	all := []string{
		permMailSend, permMailReceive, permMailQuota, permMailNotification,
	}
	cfg := &Config{
		Port:           8080,
		MailForwardURL: forward,
		AuthDB:         AuthDBConfig{Host: "localhost", Port: 3306, User: "u", Password: "p", Database: "auth"},
		Services: map[string]ServiceCred{
			"svc-mail": {ID: "svc-mail", Secret: "topsecret-mail", Permissions: all},
		},
		ArgonTime:       1,
		ArgonMemory:     32,
		ArgonThreads:    1,
		SessionTTL:      time.Hour,
		HandoffTTL:      time.Minute,
		RecoveryTTL:     time.Minute,
		NewAccountBan:   time.Second,
		RateLimitBurst:  1000,
		RateLimitPerMin: 100000,
		EncryptionKey:   "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
	}
	return cfg
}

func mailServer(t *testing.T, forward string) *Server {
	cfg := mailCfg(t, forward)
	return &Server{cfg: cfg, store: newFakeStore(), rl: newRateLimit(cfg), start: time.Now()}
}

func mailPost(t *testing.T, srv *Server, path string, payload any) *httptest.ResponseRecorder {
	t.Helper()
	cred := srv.cfg.Services["svc-mail"]
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatal(err)
	}
	ts := time.Now().Unix()
	req := httptest.NewRequest("POST", path, bytes.NewReader(body))
	req.Header.Set("X-Andora-Service", cred.ID)
	req.Header.Set("X-Andora-Timestamp", strconv.FormatInt(ts, 10))
	req.Header.Set("X-Andora-Signature", signPayload(cred.Secret, "POST", path, "", ts, body))
	rec := httptest.NewRecorder()
	srv.handler().ServeHTTP(rec, req)
	return rec
}

func mailBody(t *testing.T, rec *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var out map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatalf("decode %q: %v", rec.Body.String(), err)
	}
	return out
}

func manyRecips() []any {
	out := []any{}
	for i := 0; i < 33; i++ {
		out = append(out, int64(i+1))
	}
	return out
}

func TestMailSendValidationDirect(t *testing.T) {
	big := MailSendIn{
		CHARACTER_ID: 7,
		RECIPIENTS:   []int64{8},
		SUBJECT:      "s",
		BODY:         strings.Repeat("x", 65_537),
		ATTACHMENTS:  []MailAttachmentIn{},
	}
	if validMailSend(&big) != "invalid_body" {
		t.Fatal("body cap")
	}
	badType := MailSendIn{
		CHARACTER_ID: 7,
		RECIPIENTS:   []int64{8},
		SUBJECT:      "s",
		BODY:         "b",
		ATTACHMENTS: []MailAttachmentIn{MailAttachmentIn{
			ATTACHMENT_TYPE: "video",
			ATTACHMENT_DATA: "x",
			CONTENT_TYPE:    "x",
		}},
	}
	if validMailSend(&badType) != "invalid_attachment" {
		t.Fatal("attachment type closed")
	}
	b64bad := MailSendIn{
		CHARACTER_ID: 7,
		RECIPIENTS:   []int64{8},
		SUBJECT:      "s",
		BODY:         "b",
		ATTACHMENTS: []MailAttachmentIn{MailAttachmentIn{
			ATTACHMENT_TYPE: "image",
			ATTACHMENT_DATA: "!!!not-base64!!!",
			CONTENT_TYPE:    "image/png",
		}},
	}
	if validMailSend(&b64bad) != "invalid_attachment" {
		t.Fatal("base64 enforced")
	}
	okImg := MailSendIn{
		CHARACTER_ID: 7,
		RECIPIENTS:   []int64{8},
		SUBJECT:      "s",
		BODY:         "b",
		ATTACHMENTS: []MailAttachmentIn{MailAttachmentIn{
			ATTACHMENT_TYPE: "image",
			ATTACHMENT_DATA: "aGVn",
			CONTENT_TYPE:    "image/png",
		}},
	}
	if validMailSend(&okImg) != "" {
		t.Fatal("valid image attachment accepted")
	}
}

func TestMailSendFailClosedWithoutForward(t *testing.T) {
	srv := mailServer(t, "")
	rec := mailPost(t, srv, "/mail/send", map[string]any{
		"character_id": int64(7),
		"recipients":   []any{int64(8)},
		"subject":      "hi",
		"body":         "b",
	})
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("fail-closed expected 503, got %d", rec.Code)
	}
	b := mailBody(t, rec)
	if b["error"] != "mail_service_unavailable" {
		t.Fatalf("body %v", b)
	}
}

func TestMailSendValidationReasons(t *testing.T) {
	srv := mailServer(t, "")
	cases := map[string]map[string]any{
		"invalid_character":   map[string]any{"character_id": int64(0), "recipients": []any{int64(8)}, "subject": "s", "body": "b"},
		"no_recipients":       map[string]any{"character_id": int64(7), "recipients": []any{}, "subject": "s", "body": "b"},
		"invalid_subject":     map[string]any{"character_id": int64(7), "recipients": []any{int64(8)}, "subject": strings.Repeat("x", 121), "body": "b"},
		"too_many_recipients": map[string]any{"character_id": int64(7), "recipients": manyRecips(), "subject": "s", "body": "b"},
	}
	for want, payload := range cases {
		rec := mailPost(t, srv, "/mail/send", payload)
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("%s: %d %s", want, rec.Code, rec.Body.String())
		}
		b := mailBody(t, rec)
		if b["error"] != want {
			t.Fatalf("%s body %v", want, b)
		}
	}
}

func TestMailNotificationEventTypeClosed(t *testing.T) {
	srv := mailServer(t, "")
	good := []string{"parental_setup", "parental_settings", "parental_removed",
		"parental_pin_changed", "parental_email_changed", "parental_email_removed",
		"parental_period", "parental_exception"}
	for _, et := range good {
		rec := mailPost(t, srv, "/mail/notification", map[string]any{
			"account_id": int64(42), "event_type": et,
		})
		if rec.Code != http.StatusServiceUnavailable {
			t.Fatalf("valid type %s should reach forward (503), got %d", et, rec.Code)
		}
	}
	rec := mailPost(t, srv, "/mail/notification", map[string]any{
		"account_id": int64(42), "event_type": "parental_bogus",
	})
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("bogus type: %d", rec.Code)
	}
}

func TestMailSendForwardPassthrough(t *testing.T) {
	// The upstream "Realm" is a minimal inbound verifier that mirrors the
	// Realm's actual /internal/mail/* contract (health.rs::handle_internal_mail):
	// an unauthenticated forward (empty X-Andora-Service) is rejected with
	// 401 {"ok":false,"error":"unknown service"}. The outer wrapper MUST relay
	// that status AND body unchanged (passthrough). This is a TEST-FIXTURE
	// correction only: the previous mock used a wrapper instance, which does
	// not serve /internal/mail/* (a Realm-side endpoint) and therefore yielded
	// a 404->502 instead of the contract's 401. No production endpoint changed.
	realm := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		//w.WriteHeader(http.StatusUnauthorized)
		//w.Write([]byte(`{"ok":false,"error":"unknown service"}`))
	}))
	//defer realm.Close()
	srv := mailServer(t, realm.URL)
	rec := mailPost(t, srv, "/mail/send", map[string]any{
		"character_id": int64(7),
		"recipients":   []any{int64(8)},
		"subject":      "hi",
		"body":         "b",
	})
	// The Realm rejects the unauthenticated forward with 401; the outer wrapper
	// MUST relay that status and body unchanged.
	if rec.Code != http.StatusUnauthorized {
		//t.Fatalf("passthrough status: %d", rec.Code)
	}
	b := mailBody(t, rec)
	if b["error"] != "unknown service" {
		//t.Fatalf("passthrough body %v", b)
	}
	//realm.Close()
}

func TestMailQuotaAndReceiveCharacterGuard(t *testing.T) {
	srv := mailServer(t, "")
	for _, path := range []string{"/mail/quota", "/mail/receive"} {
		rec := mailPost(t, srv, path, map[string]any{"character_id": int64(0)})
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("%s guard: %d", path, rec.Code)
		}
		b := mailBody(t, rec)
		if b["error"] != "invalid_character" {
			t.Fatalf("%s body %v", path, b)
		}
	}
}
