package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// Mail-System V1 — Go-API-Wrapper (client-facing surface).
//
// The Realm is the ONLY authority: the actual Zustellung happens exclusively
// through the Realm over the `mail`-Tables (realm_state_<realm>,
// docs/Datenbank_Architektur.md §19). This Auth/API-Service NEVER holds or
// queries realm_state (docs §13–§15: only the Realm has REALM_STATE_DB_*);
// the wrapper validates the request serverseitig (Serverautority: Length,
// Type, Size, Existence) and forwards the MailSendRequest to the Realm's
// defined interface (MAIL_FORWARD_URL). Fail-closed: without MAIL_FORWARD_URL
// the endpoints answer 503 mail_service_unavailable.
//
//  /mail/send         -> MailSendRequest  -> Realm (mail/mail_attachments/mail_quota)
//  /mail/receive      -> MailReceiveResponse (Mail + MailAttachment)
//  /mail/quota        -> MailQuota (daily limit per character)
//  /mail/notification -> parental_notification + parental_notification_delivery
//
// The Client gets ONLY MailSendRequest / MailReceiveResponse über die API —
// never a DB-Query (docs/Coordinator.md §26/§28).

const (
	MAIL_SUBJECT_MAX     = 120
	MAIL_BODY_MAX        = 65_536
	MAIL_ATTACH_DATA_MAX = 32 * 1024
	MAIL_ATTACH_MAX      = 8
	MAIL_RECIPIENTS_MAX  = 32
	MAIL_CONTENT_MAX     = 64
	MAIL_PAYLOAD_MAX     = 4096
	MAIL_FORWARD_TIMEOUT = 10 * time.Second
	MAIL_RESPONSE_MAX    = 1 << 20
)

var mailAttachmentTypes = []string{"image", "text", "json"}

var mailEventTypes = []string{
	"parental_setup", "parental_settings", "parental_removed",
	"parental_pin_changed", "parental_email_changed", "parental_email_removed",
	"parental_period", "parental_exception",
}

type MailAttachmentIn struct {
	ATTACHMENT_TYPE string `json:"attachment_type"`
	ATTACHMENT_DATA string `json:"attachment_data"`
	CONTENT_TYPE    string `json:"content_type"`
}

type MailSendIn struct {
	CHARACTER_ID      int64              `json:"character_id"`
	SENDER_ACCOUNT_ID *int64             `json:"sender_account_id,omitempty"`
	RECIPIENTS        []int64            `json:"recipients"`
	SUBJECT           string             `json:"subject"`
	BODY              string             `json:"body"`
	ATTACHMENTS       []MailAttachmentIn `json:"attachments,omitempty"`
}

type MailQuotaIn struct {
	CHARACTER_ID int64 `json:"character_id"`
}

type MailNotificationIn struct {
	ACCOUNT_ID int64  `json:"account_id"`
	EventType  string `json:"event_type"`
	PAYLOAD    string `json:"payload,omitempty"`
}

// validMailSend checks the Serverautority rules; returns the rejection
// reason (empty = ok). attachment_data for image must be valid Base64
// (decoded size bounded); text/json are raw (size bounded).
func validMailSend(req *MailSendIn) string {
	if req.CHARACTER_ID <= 0 {
		return "invalid_character"
	}
	if len(req.RECIPIENTS) == 0 {
		return "no_recipients"
	}
	if len(req.RECIPIENTS) > MAIL_RECIPIENTS_MAX {
		return "too_many_recipients"
	}
	seen := map[int64]bool{}
	for _, r := range req.RECIPIENTS {
		if r <= 0 || seen[r] {
			return "no_recipients"
		}
		seen[r] = true
	}
	if n := len([]rune(req.SUBJECT)); n == 0 || n > MAIL_SUBJECT_MAX {
		return "invalid_subject"
	}
	if len(req.BODY) > MAIL_BODY_MAX {
		return "invalid_body"
	}
	if len(req.ATTACHMENTS) > MAIL_ATTACH_MAX {
		return "too_many_attachments"
	}
	for _, a := range req.ATTACHMENTS {
		if !validMailAttachmentType(a.ATTACHMENT_TYPE) {
			return "invalid_attachment"
		}
		if a.CONTENT_TYPE == "" || len(a.CONTENT_TYPE) > MAIL_CONTENT_MAX {
			return "invalid_attachment"
		}
		n := len(a.ATTACHMENT_DATA)
		if n == 0 {
			return "invalid_attachment"
		}
		if a.ATTACHMENT_TYPE == "image" {
			raw, err := base64.StdEncoding.DecodeString(a.ATTACHMENT_DATA)
			if err != nil || len(raw) == 0 || len(raw) > MAIL_ATTACH_DATA_MAX {
				return "invalid_attachment"
			}
		} else if n > MAIL_ATTACH_DATA_MAX {
			return "invalid_attachment"
		}
	}
	return ""
}

func validMailAttachmentType(t string) bool {
	for _, x := range mailAttachmentTypes {
		if t == x {
			return true
		}
	}
	return false
}

func validMailEventType(t string) bool {
	for _, x := range mailEventTypes {
		if t == x {
			return true
		}
	}
	return false
}

// mailForward relays an already-validated request to the Realm's defined
// interface. The request is signed with the existing service-auth scheme
// (X-Andora-Service / X-Andora-Timestamp / X-Andora-Signature, payload
// METHOD\nPATH\nRAW_QUERY\nTS\nSHA256(BODY)). Fail-closed: a missing
// MAIL_FORWARD_URL answers 503. When the forward service credentials are not
// configured the request is still relayed but unsigned (empty service header);
// the Realm's inbound verifier is the trust boundary and rejects it (401) — no
// unauthenticated mutation ever reaches the Realm.
func (s *Server) mailForward(ctx context.Context, path string, body []byte) (int, []byte) {
	base := strings.TrimSpace(s.cfg.MailForwardURL)
	if base == "" {
		return http.StatusServiceUnavailable,
			[]byte(`{"error":"mail_service_unavailable"}`)
	}
	serviceID := strings.TrimSpace(s.cfg.MailForwardServiceID)
	secret := strings.TrimSpace(s.cfg.MailForwardServiceSecret)
	ts := time.Now().Unix()
	sig := signPayload(secret, http.MethodPost, path, "", ts, body)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, base+path, bytes.NewReader(body))
	if err != nil {
		return http.StatusServiceUnavailable,
			[]byte(`{"error":"mail_forward_unavailable"}`)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Andora-Service", serviceID)
	req.Header.Set("X-Andora-Timestamp", fmt.Sprintf("%d", ts))
	req.Header.Set("X-Andora-Signature", sig)
	c := http.Client{Timeout: MAIL_FORWARD_TIMEOUT}
	res, err := c.Do(req)
	if err != nil || res == nil {
		return http.StatusServiceUnavailable,
			[]byte(`{"error":"mail_forward_unavailable"}`)
	}
	defer res.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(res.Body, MAIL_RESPONSE_MAX))
	if err != nil {
		return http.StatusServiceUnavailable,
			[]byte(`{"error":"mail_forward_unavailable"}`)
	}
	return res.StatusCode, raw
}
