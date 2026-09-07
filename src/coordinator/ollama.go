package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"strings"
	"time"
)

// chatMessage is one turn of the /api/chat conversation.
type chatMessage struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

type ollamaOptions struct {
	NumCtx      int     `json:"num_ctx"`
	Temperature float64 `json:"temperature"`
	TopP        float64 `json:"top_p"`
	TopK        int     `json:"top_k"`
}

type ollamaChatRequest struct {
	Model    string        `json:"model"`
	Messages []chatMessage `json:"messages"`
	Stream   bool          `json:"stream"`
	Options  ollamaOptions `json:"options"`
}

type ollamaResponse struct {
	Message chatMessage `json:"message"`
	Done    bool        `json:"done"`
	Error   string      `json:"error,omitempty"`
}

// ollamaClient is the thin, sole Ollama access point (§3). The
// coordinator owns queueing, load limits, timeouts and retries; the
// realm never talks to Ollama directly.
type ollamaClient struct {
	base    string
	model   string
	opts    ollamaOptions
	timeout time.Duration
	http    *http.Client
}

func newOllamaClient(cfg *Config) *ollamaClient {
	return &ollamaClient{
		base:  cfg.OllamaURL,
		model: cfg.OllamaModel,
		opts: ollamaOptions{
			NumCtx:      cfg.NumCtx,
			Temperature: cfg.Temperature,
			TopP:        cfg.TopP,
			TopK:        cfg.TopK,
		},
		timeout: cfg.OllamaTimeout,
		http:    &http.Client{Timeout: cfg.OllamaTimeout},
	}
}

// Failure classes the queue maps to coordinator result codes.
const (
	chatOK          = ""
	chatTimeout     = CodeTimeout
	chatUnavailable = CodeAIUnavailable
)

// chat runs one non-streaming generation. It returns the model answer
// and one of chatOK / chatTimeout / chatUnavailable. An empty answer is
// NOT an error here; the validator classifies it (sections 10/12).
func (o *ollamaClient) chat(ctx context.Context, messages []chatMessage) (string, string) {
	reqBody := ollamaChatRequest{
		Model:    o.model,
		Messages: messages,
		Stream:   false,
		Options:  o.opts,
	}
	raw, err := json.Marshal(reqBody)
	if err != nil {
		return "", chatUnavailable
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, o.base+"/api/chat", bytes.NewReader(raw))
	if err != nil {
		return "", chatUnavailable
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := o.http.Do(req)
	if err != nil {
		if isTimeout(err) {
			return "", chatTimeout
		}
		return "", chatUnavailable
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 2*1024*1024))
	if err != nil {
		return "", chatUnavailable
	}
	if resp.StatusCode != http.StatusOK {
		msg := strings.TrimSpace(string(body))
		if len(msg) > 300 {
			msg = msg[:300]
		}
		// A 5xx / model-load failure is "unavailable right now".
		return "", chatUnavailable
	}
	var out ollamaResponse
	if err := json.Unmarshal(body, &out); err != nil {
		return "", chatUnavailable
	}
	return out.Message.Content, chatOK
}

// ping probes Ollama availability (/api/version). Used for /status and
// startup; individual jobs still get their own timeout handling.
func (o *ollamaClient) ping(ctx context.Context) bool {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, o.base+"/api/version", nil)
	if err != nil {
		return false
	}
	resp, err := o.http.Do(req)
	if err != nil {
		return false
	}
	defer resp.Body.Close()
	return resp.StatusCode == http.StatusOK
}

// isTimeout reports whether an error is a client-side timeout (the
// only case that maps to CodeTimeout; every other transport failure is
// AI_UNVAILABLE).
func isTimeout(err error) bool {
	if err == nil {
		return false
	}
	if ne, ok := err.(net.Error); ok && ne.Timeout() {
		return true
	}
	return strings.Contains(err.Error(), "context deadline exceeded")
}
