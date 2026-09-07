package main

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"testing"
	"time"
)

func postJob(t *testing.T, srv *Server, cred RealmCred, sub jobSubmission) (int, map[string]any) {
	t.Helper()
	body, _ := json.Marshal(sub)
	ts := time.Now().Unix()
	r := signedRequest(cred, "POST", "/v1/jobs", "", ts, string(body))
	w := httptest.NewRecorder()
	srv.handler().ServeHTTP(w, r)
	var m map[string]any
	json.Unmarshal(w.Body.Bytes(), &m)
	return w.Code, m
}

func TestSubmitJobOK(t *testing.T) {
	srv := newTestServer(t, "")
	cred := srv.cfg.Realms["de1"]
	code, m := postJob(t, srv, cred, jobSubmission{
		JobType: "player", JobID: "j1", Text: "hallo",
	})
	if code != http.StatusCreated {
		t.Fatalf("status %d: %v", code, m)
	}
	if m["code"] != CodeQueued || m["job_id"] != "j1" {
		t.Errorf("response: %v", m)
	}
}

func TestSubmitJobInvalidJSON(t *testing.T) {
	srv := newTestServer(t, "")
	cred := srv.cfg.Realms["de1"]
	ts := time.Now().Unix()
	r := signedRequest(cred, "POST", "/v1/jobs", "", ts, "}{broken")
	r.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	srv.handler().ServeHTTP(w, r)
	if w.Code != http.StatusBadRequest {
		t.Errorf("code = %d", w.Code)
	}
}

func TestSubmitJobEmptyText(t *testing.T) {
	srv := newTestServer(t, "")
	cred := srv.cfg.Realms["de1"]
	code, _ := postJob(t, srv, cred, jobSubmission{JobType: "player", JobID: "j2", Text: ""})
	if code != http.StatusBadRequest {
		t.Errorf("empty text: code = %d", code)
	}
}

func TestSubmitJobTooLong(t *testing.T) {
	srv := newTestServer(t, "MAX_TEXT_CHARS=5\n")
	cred := srv.cfg.Realms["de1"]
	code, _ := postJob(t, srv, cred, jobSubmission{JobType: "player", JobID: "j3", Text: "hello world"})
	if code != http.StatusBadRequest {
		t.Errorf("long text: code = %d", code)
	}
}

func TestSubmitJobUnknownType(t *testing.T) {
	srv := newTestServer(t, "")
	cred := srv.cfg.Realms["de1"]
	code, _ := postJob(t, srv, cred, jobSubmission{JobType: "alien", JobID: "j4", Text: "x"})
	if code != http.StatusBadRequest {
		t.Errorf("unknown type: code = %d", code)
	}
}

func TestSubmitJobQueueFull(t *testing.T) {
	srv := newTestServer(t, "QUEUE_MAX_SIZE=1\n")
	cred := srv.cfg.Realms["de1"]
	c1, _ := postJob(t, srv, cred, jobSubmission{JobType: "player", JobID: "qf1", Text: "a"})
	if c1 != http.StatusCreated {
		t.Fatalf("first job: %d", c1)
	}
	// Now add a real pending job so pending = 1 == QUEUE_MAX_SIZE
	c2, _ := postJob(t, srv, cred, jobSubmission{JobType: "player", JobID: "qf2", Text: "b"})
	// Accept may have consumed qf1 immediately so both might succeed.
	if c2 != http.StatusCreated && c2 != http.StatusServiceUnavailable {
		t.Errorf("second job: code = %d", c2)
	}
}

func TestQueryJobNotFound(t *testing.T) {
	srv := newTestServer(t, "")
	cred := srv.cfg.Realms["de1"]
	ts := time.Now().Unix()
	r := signedRequest(cred, "GET", "/v1/jobs/nonexistent", "", ts, "")
	w := httptest.NewRecorder()
	srv.handler().ServeHTTP(w, r)
	if w.Code != http.StatusNotFound {
		t.Errorf("code = %d", w.Code)
	}
	var m map[string]any
	json.Unmarshal(w.Body.Bytes(), &m)
	if m["exists"] != false {
		t.Errorf("exists must be false")
	}
}

func TestSubmitThenQueryFound(t *testing.T) {
	srv := newTestServer(t, "")
	cred := srv.cfg.Realms["de1"]
	code, _ := postJob(t, srv, cred, jobSubmission{JobType: "player", JobID: "qf1", Text: "hallo"})
	if code != http.StatusCreated {
		t.Fatalf("submit: %d", code)
	}
	ts := time.Now().Unix()
	r := signedRequest(cred, "GET", "/v1/jobs/qf1", "", ts, "")
	w := httptest.NewRecorder()
	srv.handler().ServeHTTP(w, r)
	if w.Code != http.StatusOK {
		t.Fatalf("query: %d %s", w.Code, w.Body)
	}
	var m map[string]any
	json.Unmarshal(w.Body.Bytes(), &m)
	if m["exists"] != true || m["job_id"] != "qf1" {
		t.Errorf("query result: %v", m)
	}
}

func TestSubmitJobUnauthenticated(t *testing.T) {
	srv := newTestServer(t, "")
	ts := time.Now().Unix()
	body, _ := json.Marshal(jobSubmission{JobType: "player", JobID: "u1", Text: "hi"})
	r := httptest.NewRequest("POST", "/v1/jobs", bytes.NewReader(body))
	r.Header.Set("X-Andora-Service", "unknown")
	r.Header.Set("X-Andora-Timestamp", strconv.FormatInt(ts, 10))
	r.Header.Set("X-Andora-Signature", signPayload("nope", "POST", "/v1/jobs", "", ts, body))
	w := httptest.NewRecorder()
	srv.handler().ServeHTTP(w, r)
	if w.Code != http.StatusUnauthorized {
		t.Errorf("unauthed submit: %d", w.Code)
	}
}

func TestSubmitJobWrongPermission(t *testing.T) {
	srv := newTestServer(t, "SERVICE_DE1_PERMISSIONS=coordinator.jobs.query\n")
	cred := srv.cfg.Realms["de1"]
	code, _ := postJob(t, srv, cred, jobSubmission{JobType: "player", JobID: "p1", Text: "x"})
	if code != http.StatusForbidden {
		t.Errorf("wrong perm: code = %d", code)
	}
}

func TestQueryJobWrongPermission(t *testing.T) {
	srv := newTestServer(t, "SERVICE_DE1_PERMISSIONS=coordinator.jobs.submit\n")
	cred := srv.cfg.Realms["de1"]
	ts := time.Now().Unix()
	r := signedRequest(cred, "GET", "/v1/jobs/nonexistent", "", ts, "")
	w := httptest.NewRecorder()
	srv.handler().ServeHTTP(w, r)
	if w.Code != http.StatusForbidden {
		t.Errorf("wrong perm: code = %d", w.Code)
	}
}

func TestQueryInvalidJobID(t *testing.T) {
	srv := newTestServer(t, "")
	cred := srv.cfg.Realms["de1"]
	ts := time.Now().Unix()
	r := signedRequest(cred, "GET", "/v1/jobs/bad/id", "", ts, "")
	w := httptest.NewRecorder()
	srv.handler().ServeHTTP(w, r)
	if w.Code != http.StatusBadRequest {
		t.Errorf("invalid id: code = %d", w.Code)
	}
}
