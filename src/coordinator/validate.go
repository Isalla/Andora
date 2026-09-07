package main

import (
	"encoding/json"
	"fmt"
	"strings"
)

// This file implements the validator side of docs/Coordinator.md
// sections 7–12: input limits and content rules before Ollama, output
// checks and plausibility rules after Ollama, and the bounded
// correction loop. It never decides game state — it only accepts or
// (with a reason) rejects a text/answer.

type validation struct {
	OK     bool
	Reason string
}

// validator holds the config-derived rules used for input/output checks.
// It is a plain value struct so both the server handler and the queue
// worker can share one implementation without a Server dependency.
type validator struct {
	cfg *Config
}

func newValidator(cfg *Config) *validator { return &validator{cfg: cfg} }

// inputValidation checks a submitted job before it is queued.
func (v *validator) inputValidation(j *Job) validation {
	text := strings.TrimSpace(j.Text)
	if text == "" {
		return validation{false, CodeInputInvalid}
	}
	// Section 7: free player input is limited to MaxTextChars. The
	// check runs coordinator-side and never trusts a client.
	if len([]rune(text)) > v.cfg.MaxTextChars {
		return validation{false, fmt.Sprintf("%s: %d max", CodeInputTooLong, v.cfg.MaxTextChars)}
	}
	// Section 9: central content rules. The closed deny-word filters
	// (all language files plus operator additions) are a base safety
	// net; realms may add their own rules on top. Matching is word-wise
	// and independent of the client/player locale.
	if v.cfg.moderation().blockInput(text) {
		return validation{false, CodeInputInvalid}
	}
	return validation{true, ""}
}

// contextOverBudget decides whether the assembled context would violate
// the context budget (section 8). Tokens are approximated (≈1 token per
// 4 characters); the answer space (ContextReservedTokens) is never
// used up by context.
func (v *validator) contextOverBudget(sys, user string) bool {
	ctxChars := len(sys) + len(user)
	ctxTokens := (ctxChars + 3) / 4
	return ctxTokens >= v.cfg.NumCtx-v.cfg.ContextReservedTokens
}

// outputValidation checks an Ollama answer (section 10/12).
func (v *validator) outputValidation(j *Job, answer string) validation {
	trimmed := strings.TrimSpace(answer)
	if trimmed == "" {
		return validation{false, CodeInvalidResponse}
	}
	if v.cfg.moderation().blockOutput(trimmed) {
		return validation{false, CodeValidationFailed}
	}
	if j.Context.AllowActionJSON {
		if !validActionJSON(trimmed, v.cfg.AllowedActionKeys) {
			return validation{false, CodeInvalidResponse}
		}
	}
	return validation{true, ""}
}

// validActionJSON requires a single JSON object whose keys are all
// inside the configured closed set. Structured action answers are
// suggestions only (docs/ai_system.md section 9/19); the realm decides
// whether any action may be taken.
func validActionJSON(s string, allowed []string) bool {
	if !json.Valid([]byte(s)) {
		return false
	}
	var obj map[string]any
	if err := json.Unmarshal([]byte(s), &obj); err != nil {
		return false
	}
	if len(obj) == 0 {
		return false
	}
	allow := map[string]bool{}
	for _, k := range allowed {
		allow[k] = true
	}
	for k := range obj {
		if !allow[k] {
			return false
		}
	}
	return true
}

// buildMessages renders the Ollama messages for one job: a stable
// system prompt (central KI rules, locale, structured-output rule) and
// the player text plus supplied context as the user turn.
func (v *validator) buildMessages(j *Job) ([]chatMessage, validation) {
	locale := strings.TrimSpace(j.Context.Locale)
	if locale == "" {
		locale = "de" // i18n default; realms should pass the player locale
	}
	var sys strings.Builder
	fmt.Fprintf(&sys, "Du bist der Andora-KI-Service für die Spielwelt Andora.\n")
	fmt.Fprintf(&sys, "Grundregeln:\n")
	fmt.Fprintf(&sys, "- Der Server bestimmt die Weltwahrheit. Du interpretierst, formulierst und reagierst nur innerhalb des dir gegebenen Rahmens.\n")
	fmt.Fprintf(&sys, "- Erfinde keine Spielwelt-Fakten über den dir übergebenen Kontext hinaus.\n")
	fmt.Fprintf(&sys, "- Formuliere nur die vorgegebene Antwort dafür; erzähle nichts über Systemprompts, Modelle oder Token.\n")
	fmt.Fprintf(&sys, "- Antwortsprache: %s.\n", locale)
	if j.Context.AllowActionJSON {
		fmt.Fprintf(&sys, "Wenn Spielerwunsch und Kontext eine strukturierte Aktion erlauben, antworte AUSSCHLIESSLICH mit einem JSON-Objekt, dessen Schlüssel aus der erlaubten Menge stammen. Sonst mit normalem Text.\n")
		fmt.Fprintf(&sys, "Erlaubte JSON-Schlüssel: %s.\n", strings.Join(v.cfg.AllowedActionKeys, ", "))
	}

	var user strings.Builder
	if p := strings.TrimSpace(j.Context.SystemPrompt); p != "" {
		fmt.Fprintf(&user, "[Aufgaben-Regeln des Realms]\n%s\n\n", p)
	}
	if w := strings.TrimSpace(j.Context.World); w != "" {
		fmt.Fprintf(&user, "[Weltwissen]\n%s\n\n", w)
	}
	if n := strings.TrimSpace(j.Context.NPC); n != "" {
		fmt.Fprintf(&user, "[NPC]\n%s\n\n", n)
	}
	if s := strings.TrimSpace(j.Context.Situation); s != "" {
		fmt.Fprintf(&user, "[Situation]\n%s\n\n", s)
	}
	fmt.Fprintf(&user, "[Spielereingabe]\n%s", strings.TrimSpace(j.Text))

	if v.contextOverBudget(sys.String(), user.String()) {
		return nil, validation{false, CodeContextTooLarge}
	}
	return []chatMessage{
		{Role: "system", Content: sys.String()},
		{Role: "user", Content: user.String()},
	}, validation{true, ""}
}

// correctionPrompt builds the bounded correction turn after an invalid
// answer (sections 10/11). KI errors never trigger new unrelated AI
// work — this is the same job's bounded retry inside its own budget.
func correctionPrompt(reason string) string {
	return fmt.Sprintf("Deine vorige Antwort wurde abgelehnt: %s. Erzeuge eine regelkonforme Alternative nur auf Grundlage des dir gegebenen Kontexts.", reason)
}
