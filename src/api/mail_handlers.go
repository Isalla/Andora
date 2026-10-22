package main

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"time"
)

// Mail-System V1 — endpoint handlers (Go-API-Wrapper).
//
// Spieler-Endpunkte validieren die Spielersession (s.store.ValidateSession),
// niemals die Service-Permission. `account_id` wird ausschließlich aus der
// validierten Session abgeleitet; `sender_account_id`/`account_id` aus dem
// Body werden ignoriert. Der Realm prüft zusätzlich die Charaktereigentum.
// Service-Endpunkte (`/mail/send`) behalten `authorize` (mail.send) und sind
// in V1 unbenutzt (der Realm erzeugt Refunds selbst). `/mail/notification`
// gehört zu Parental und ist nicht mehr Teil des Mail-Pfads.
//
// Die Realm-Antwort wird unverändert durchgereicht (status + JSON body).
// Fail-closed: ohne MAIL_FORWARD_URL antwortet der Wrapper 503.

const mailHandlerTimeout = 15 * time.Second

// ── Spieler-Request-Typen ──────────────────────────────────────────────────

type MailListIn struct {
	SESSION_ID   string `json:"session_id"`
	CHARACTER_ID int64  `json:"character_id"`
	ACCOUNT_ID   int    `json:"account_id"`
}

type MailReadIn struct {
	SESSION_ID string `json:"session_id"`
	CHARACTER_ID int64  `json:"character_id"`
	ACCOUNT_ID   int    `json:"account_id"`
	MAIL_ID    string `json:"mail_id"`
}

type MailClaimIn struct {
	SESSION_ID string `json:"session_id"`
	CHARACTER_ID int64  `json:"character_id"`
	ACCOUNT_ID   int    `json:"account_id"`
	MAIL_ID    string `json:"mail_id"`
}

// ── Spieler-Endpunkte (Session-geprüft) ────────────────────────────────────

func (s *Server) handleMailList(w http.ResponseWriter, r *http.Request) {
	body, ok := s.readPlayerBody(w, r)
	if !ok {
		return
	}
	var req MailListIn
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	acc, ok := s.requirePlayerSession(w, r, req.SESSION_ID, req.CHARACTER_ID)
	if !ok {
		return
	}
	req.ACCOUNT_ID = acc
	canonical, err := json.Marshal(&req)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "encode request")
		return
	}
	s.mailRelay(w, r, "/internal/mail/list", canonical)
}

func (s *Server) handleMailRead(w http.ResponseWriter, r *http.Request) {
	body, ok := s.readPlayerBody(w, r)
	if !ok {
		return
	}
	var req MailReadIn
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	acc, ok := s.requirePlayerSession(w, r, req.SESSION_ID, req.CHARACTER_ID)
	if !ok {
		return
	}
	req.ACCOUNT_ID = acc
	canonical, err := json.Marshal(&req)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "encode request")
		return
	}
	s.mailRelay(w, r, "/internal/mail/read", canonical)
}

func (s *Server) handleMailClaim(w http.ResponseWriter, r *http.Request) {
	body, ok := s.readPlayerBody(w, r)
	if !ok {
		return
	}
	var req MailClaimIn
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	acc, ok := s.requirePlayerSession(w, r, req.SESSION_ID, req.CHARACTER_ID)
	if !ok {
		return
	}
	req.ACCOUNT_ID = acc
	canonical, err := json.Marshal(&req)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "encode request")
		return
	}
	s.mailRelay(w, r, "/internal/mail/claim", canonical)
}

// ── Service-Endpunkte (Service-Auth; V1 mostly unbenutzt, validation kept) ──

func (s *Server) handleMailSend(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permMailSend)
	if !ok {
		return
	}
	var req MailSendIn
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if reason := validMailSend(&req); reason != "" {
		writeError(w, http.StatusBadRequest, reason)
		return
	}
	// In V1 unbenutzt: the Realm generates refunds itself. The endpoint stays as
	// a service-route; the validated payload is relayed for completeness.
	canonical, err := json.Marshal(&req)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "encode request")
		return
	}
	s.mailRelay(w, r, "/internal/mail/send", canonical)
}

func (s *Server) handleMailQuota(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permMailQuota)
	if !ok {
		return
	}
	var req MailQuotaIn
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if req.CHARACTER_ID <= 0 {
		writeError(w, http.StatusBadRequest, "invalid_character")
		return
	}
	canonical, err := json.Marshal(&req)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "encode request")
		return
	}
	s.mailRelay(w, r, "/internal/mail/quota", canonical)
}

func (s *Server) handleMailReceive(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permMailReceive)
	if !ok {
		return
	}
	var req MailQuotaIn
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if req.CHARACTER_ID <= 0 {
		writeError(w, http.StatusBadRequest, "invalid_character")
		return
	}
	canonical, err := json.Marshal(&req)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "encode request")
		return
	}
	s.mailRelay(w, r, "/internal/mail/receive", canonical)
}

func (s *Server) handleMailNotification(w http.ResponseWriter, r *http.Request) {
	body, _, ok := s.authorize(w, r, permMailNotification)
	if !ok {
		return
	}
	var req MailNotificationIn
	if err := json.Unmarshal(body, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}
	if !validMailEventType(req.EventType) {
		writeError(w, http.StatusBadRequest, "invalid_event_type")
		return
	}
	canonical, err := json.Marshal(&req)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "encode request")
		return
	}
	s.mailRelay(w, r, "/internal/mail/notification", canonical)
}

// ── Hilfsfunktionen ────────────────────────────────────────────────────────

// readPlayerBody liest den Request-Body (ohne Service-Autorisierung).
func (s *Server) readPlayerBody(w http.ResponseWriter, r *http.Request) ([]byte, bool) {
	body, err := io.ReadAll(io.LimitReader(r.Body, 1<<20))
	if err != nil {
		writeError(w, http.StatusBadRequest, "unreadable body")
		return nil, false
	}
	return body, true
}

// requirePlayerSession validiert die Spielersession und leitet account_id ab.
// Der Realm prüft zusätzlich die Charaktereigentum.
func (s *Server) requirePlayerSession(w http.ResponseWriter, r *http.Request, sessionID string, characterID int64) (int, bool) {
	if sessionID == "" {
		writeError(w, http.StatusUnauthorized, "session required")
		return 0, false
	}
	ctx, cancel := context.WithTimeout(r.Context(), mailHandlerTimeout)
	defer cancel()
	sess, err := s.store.ValidateSession(ctx, sessionID)
	if err != nil || sess == nil {
		writeError(w, http.StatusUnauthorized, "invalid session")
		return 0, false
	}
	if sess.AccountID <= 0 {
		writeError(w, http.StatusUnauthorized, "invalid session")
		return 0, false
	}
	if characterID <= 0 {
		writeError(w, http.StatusBadRequest, "invalid_character")
		return 0, false
	}
	return sess.AccountID, true
}

// mailRelay forwards a validated body and relays the Realm's answer
// unchanged (status + JSON body). Fail-closed on missing/failed forward.
func (s *Server) mailRelay(w http.ResponseWriter, r *http.Request, path string, body []byte) {
	ctx, cancel := context.WithTimeout(r.Context(), mailHandlerTimeout)
	defer cancel()
	status, raw := s.mailForward(ctx, path, body)
	if status == 0 {
		writeError(w, http.StatusBadGateway, "mail_forward_failed")
		return
	}
	var out any
	if err := json.Unmarshal(raw, &out); err != nil {
		writeError(w, http.StatusBadGateway, "mail_forward_bad_response")
		return
	}
	writeJSON(w, status, out)
}
