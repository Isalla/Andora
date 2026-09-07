package main

import (
	"strings"
	"testing"
	"time"
)

func tCfg(t *testing.T, env string) *Config {
	t.Helper()
	return loadEnvString(t, env)
}

func TestInputValidationEmpty(t *testing.T) {
	cfg := tCfg(t, minimalEnv)
	v := newValidator(cfg)
	j := &Job{Text: ""}
	if rv := v.inputValidation(j); rv.OK {
		t.Error("empty input must fail")
	}
}

func TestInputValidationTooLong(t *testing.T) {
	cfg := tCfg(t, minimalEnv+"MAX_TEXT_CHARS=10\n")
	v := newValidator(cfg)
	j := &Job{Text: strings.Repeat("a", 20)}
	if rv := v.inputValidation(j); rv.OK || !strings.HasPrefix(rv.Reason, CodeInputTooLong) {
		t.Errorf("long input: %+v", rv)
	}
}

func TestInputValidationDenyWord(t *testing.T) {
	cfg := tCfg(t, minimalEnv+"INPUT_DENY_WORDS=bad\n")
	v := newValidator(cfg)
	j := &Job{Text: "this is bad"}
	if rv := v.inputValidation(j); rv.OK || rv.Reason != CodeInputInvalid {
		t.Errorf("deny word: %+v", rv)
	}
}

func TestOutputValidationEmpty(t *testing.T) {
	cfg := tCfg(t, minimalEnv)
	v := newValidator(cfg)
	j := &Job{}
	if rv := v.outputValidation(j, ""); rv.OK {
		t.Error("empty output must fail")
	}
}

func TestOutputValidationOK(t *testing.T) {
	cfg := tCfg(t, minimalEnv)
	v := newValidator(cfg)
	j := &Job{}
	if rv := v.outputValidation(j, "Hallo!"); !rv.OK {
		t.Errorf("valid output: %+v", rv)
	}
}

func TestOutputValidationDenyWord(t *testing.T) {
	cfg := tCfg(t, minimalEnv+"OUTPUT_DENY_WORDS=secret\n")
	v := newValidator(cfg)
	j := &Job{}
	if rv := v.outputValidation(j, "the secret is here"); rv.OK || rv.Reason != CodeValidationFailed {
		t.Errorf("deny word: %+v", rv)
	}
}

func TestContextOverBudget(t *testing.T) {
	cfg := tCfg(t, minimalEnv+"OLLAMA_NUM_CTX=100\nCONTEXT_RESERVED_TOKENS=20\n")
	v := newValidator(cfg)
	// 80 tokens ≈ 320 chars available before over-budget
	short := strings.Repeat("x", 100)
	long := strings.Repeat("x", 400)
	if v.contextOverBudget(short, short) {
		t.Error("short context must fit")
	}
	if !v.contextOverBudget(long, long) {
		t.Error("large context must be over budget")
	}
}

func TestBuildMessagesIncludesContext(t *testing.T) {
	cfg := tCfg(t, minimalEnv)
	v := newValidator(cfg)
	j := &Job{
		Text: "Question?",
		Context: jobContext{
			SystemPrompt: "Be nice",
			World:        "Fantasy world",
			NPC:          "Elf",
			Situation:    "Forest",
			Locale:       "en",
		},
		CreatedAt: time.Now(),
	}
	msgs, rv := v.buildMessages(j)
	if !rv.OK {
		t.Fatalf("buildMessages: %+v", rv)
	}
	if len(msgs) != 2 {
		t.Fatalf("msgs = %d", len(msgs))
	}
	if msgs[0].Role != "system" || msgs[1].Role != "user" {
		t.Errorf("roles: %v", msgs)
	}
	if !strings.Contains(msgs[0].Content, "Andora") || !strings.Contains(msgs[0].Content, "en") {
		t.Errorf("system prompt wrong:\n%s", msgs[0].Content)
	}
	user := msgs[1].Content
	if !strings.Contains(user, "Be nice") || !strings.Contains(user, "Fantasy world") ||
		!strings.Contains(user, "Elf") || !strings.Contains(user, "Forest") ||
		!strings.Contains(user, "Question?") {
		t.Errorf("user prompt missing context:\n%s", user)
	}
}

func TestBuildMessagesActionJSONAllowedKeys(t *testing.T) {
	cfg := tCfg(t, minimalEnv+"ALLOWED_ACTION_KEYS=intent,target\n")
	v := newValidator(cfg)
	j := &Job{
		Text:      "test",
		Context:   jobContext{AllowActionJSON: true, Locale: "de"},
		CreatedAt: time.Now(),
	}
	msgs, rv := v.buildMessages(j)
	if !rv.OK || !strings.Contains(msgs[0].Content, "JSON-Schlüssel: intent, target") {
		t.Errorf("action keys not embedded:\n%s", msgs[0].Content)
	}
}

func TestCodeText(t *testing.T) {
	cases := []struct{ code, want string }{
		{CodeQueueFull, "Koordinator-Warteschlange voll"},
		{CodeAIUnavailable, "KI-Dienst nicht verfügbar"},
		{CodeTimeout, "Zeitüberschreitung"},
		{"unknown_code", "unknown_code"},
	}
	for _, c := range cases {
		if got := codeText(c.code); got != c.want {
			t.Errorf("codeText(%q) = %q", c.code, got)
		}
	}
}

func TestValidJobType(t *testing.T) {
	cfg := tCfg(t, minimalEnv)
	for _, ty := range []string{"player", "raidboss", "craft", "npc", "event", "background", "world"} {
		if !validJobType(ty, cfg) {
			t.Errorf("validJobType(%q) must be true", ty)
		}
	}
	if validJobType("unknown", cfg) {
		t.Error("validJobType(unknown) must be false")
	}
}

func TestNewJobFromPayload(t *testing.T) {
	cfg := tCfg(t, minimalEnv)
	j, err := NewJobFromPayload(jobSubmission{JobType: "player", JobID: "j1", Text: "hi"}, cfg, time.Now())
	if err != nil {
		t.Fatal(err)
	}
	if j.JobType != "player" || j.JobID != "j1" {
		t.Errorf("job = %+v", j)
	}
	if j.Status != jobPending || j.Attempts != 0 {
		t.Errorf("initial status/attempts wrong: %v", j.Status)
	}
	if j.MaxAttempts != cfg.MaxAttempts {
		t.Errorf("MaxAttempts = %d", j.MaxAttempts)
	}
}

func TestNewJobFromPayloadUnknownType(t *testing.T) {
	_, err := NewJobFromPayload(jobSubmission{JobType: "alien"}, tCfg(t, minimalEnv), time.Now())
	if err == nil {
		t.Error("unknown job type must fail")
	}
}

func TestFileNameRoundTrip(t *testing.T) {
	ts := time.Date(2026, 9, 2, 14, 20, 15, 347_000_000, time.Local)
	fn := fileName(ts, "raidboss", "4711")
	if fn != "20260902T142015.347_raidboss-4711.json" {
		t.Errorf("fileName = %q", fn)
	}
	gotTS, gotType, gotID, ok := parseFileName(fn)
	if !ok {
		t.Fatal("parseFileName failed")
	}
	if !gotTS.Equal(ts) || gotType != "raidboss" || gotID != "4711" {
		t.Errorf("parseFileName = %v %q %q", gotTS, gotType, gotID)
	}
}

func TestValidJobID(t *testing.T) {
	if validJobID("abc-123") != true {
		t.Error("validJobID(abc-123) must be true")
	}
	if validJobID("") || validJobID("abc/123") {
		t.Error("invalid job ids must be rejected")
	}
}

func TestCooldownKey(t *testing.T) {
	j := &Job{PlayerID: "p42"}
	if got := j.cooldownKey(); got != "p42" {
		t.Errorf("cooldownKey = %q, want p42", got)
	}
	j = &Job{PlayerID: "", CharacterID: "c7"}
	if got := j.cooldownKey(); got != "c7" {
		t.Errorf("cooldownKey fallback = %q, want c7", got)
	}
}
