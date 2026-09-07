package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func testOllamaClient(t *testing.T, url string) *ollamaClient {
	t.Helper()
	cfg := loadEnvString(t, minimalEnv)
	cfg.OllamaURL = url
	cfg.OllamaModel = "m"
	cfg.OllamaTimeout = 0 // no client timeout; httptest answers fast
	return newOllamaClient(cfg)
}

func TestChatOK(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/api/chat" {
			t.Errorf("unexpected request %s %s", r.Method, r.URL.Path)
		}
		var req ollamaChatRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			t.Error(err)
		}
		if req.Stream || req.Model != "m" || len(req.Messages) != 2 {
			t.Errorf("request wrong: %+v", req)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"message": map[string]any{"role": "assistant", "content": "Antwort"},
			"done":    true,
		})
	}))
	defer ts.Close()

	c := testOllamaClient(t, ts.URL)
	ans, cls := c.chat(context.Background(), []chatMessage{
		{Role: "system", Content: "sys"},
		{Role: "user", Content: "hi"},
	})
	if cls != chatOK {
		t.Fatalf("class = %q", cls)
	}
	if ans != "Antwort" {
		t.Errorf("answer = %q", ans)
	}
}

func TestChatNon200(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer ts.Close()
	c := testOllamaClient(t, ts.URL)
	if _, cls := c.chat(context.Background(), nil); cls != chatUnavailable {
		t.Errorf("class = %q, want AI_UNAVAILABLE", cls)
	}
}

func TestChatConnectionRefused(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	url := ts.URL
	ts.Close()
	c := testOllamaClient(t, url)
	if _, cls := c.chat(context.Background(), nil); cls != chatUnavailable {
		t.Errorf("class = %q, want AI_UNAVAILABLE", cls)
	}
}

func TestChatTimeout(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = r.Context() // block until server closes
	}))
	defer ts.Close()
	cfg := loadEnvString(t, minimalEnv)
	cfg.OllamaURL = ts.URL
	cfg.OllamaModel = "m"
	cfg.OllamaTimeout = 1 // nanoseconds → immediately with context timeout
	c := newOllamaClient(cfg)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, cls := c.chat(ctx, nil); cls != chatTimeout {
		t.Errorf("class = %q, want TIMEOUT", cls)
	}
}

func TestPing(t *testing.T) {
	var requested bool
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/version" {
			requested = true
			_ = json.NewEncoder(w).Encode(map[string]any{"version": "0.1"})
			return
		}
		w.WriteHeader(http.StatusNotFound)
	}))
	defer ts.Close()
	c := testOllamaClient(t, ts.URL)
	if !c.ping(context.Background()) {
		t.Error("ping must succeed")
	}
	if !requested {
		t.Error("/api/version not requested")
	}
}

func TestPingDown(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	url := ts.URL
	ts.Close()
	c := testOllamaClient(t, url)
	if c.ping(context.Background()) {
		t.Error("ping must fail when service is down")
	}
}

func TestIsTimeout(t *testing.T) {
	if isTimeout(nil) {
		t.Error("nil must not be a timeout")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 1*time.Nanosecond)
	defer cancel()
	<-ctx.Done()
	if !isTimeout(ctx.Err()) {
		t.Error("context deadline exceeded must classify as timeout")
	}
}
