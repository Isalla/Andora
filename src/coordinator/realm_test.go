package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestRealmDeliverSkippedWithoutResultURL(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv)
	c := newRealmClient(cfg)
	if err := c.deliver(context.Background(), cfg.Realms["de1"], realmResult{
		RealmID: "de1", JobID: "j1", Status: jobCompleted, Result: "ok",
	}); err != nil {
		t.Fatalf("deliver without result URL must be a no-op: %v", err)
	}
}

func TestRealmDeliverSkippedWithoutIdentity(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv)
	cfg.CoordinatorServiceID = ""
	c := newRealmClient(cfg)
	cred := cfg.Realms["de1"]
	cred.ResultURL = "http://127.0.0.1:1/x"
	if err := c.deliver(context.Background(), cred, realmResult{RealmID: "de1", JobID: "j1"}); err != nil {
		t.Fatalf("deliver without own identity must be a no-op: %v", err)
	}
}

func TestRealmDeliverSignedAndParsed(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv+minimalEnvCoordinatorIdentity)
	c := newRealmClient(cfg)
	cred := cfg.Realms["de1"]

	var received realmResult
	var verified bool
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&received); err != nil {
			t.Error(err)
		}
		if r.Header.Get("X-Andora-Service") != cfg.CoordinatorServiceID {
			t.Error("missing service header")
		}
		verified = true
		w.WriteHeader(http.StatusOK)
	}))
	defer ts.Close()
	cred.ResultURL = ts.URL

	if err := c.deliver(context.Background(), cred, realmResult{
		RealmID:   "de1",
		JobID:     "j1",
		JobType:   "player",
		Status:    jobFailed,
		ErrorCode: CodeCooldown,
	}); err != nil {
		t.Fatalf("deliver: %v", err)
	}
	if !verified {
		t.Fatal("realm never received the callback")
	}
	if received.JobID != "j1" || received.Status != jobFailed || received.ErrorCode != CodeCooldown {
		t.Errorf("received: %+v", received)
	}
}

func TestRealmDeliverNon2xx(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv+minimalEnvCoordinatorIdentity)
	c := newRealmClient(cfg)
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer ts.Close()
	cred := cfg.Realms["de1"]
	cred.ResultURL = ts.URL
	if err := c.deliver(context.Background(), cred, realmResult{RealmID: "de1", JobID: "j1", Status: jobCompleted}); err == nil {
		t.Error("non-2xx must surface as an error")
	}
}

const minimalEnvCoordinatorIdentity = "COORDINATOR_SERVICE_ID=coordinator-service\n" +
	"COORDINATOR_SERVICE_SECRET=coordinator-secret\n"
