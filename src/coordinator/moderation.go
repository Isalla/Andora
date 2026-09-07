package main

import (
	"os"
	"path/filepath"
	"strings"
	"unicode"
	"unicode/utf8"
)

// moderation holds the deny-word rules for input and output filtering.
// The rules come from two sources that are merged at load time:
//
//   - the filter directory (COORDINATOR_FILTER_DIR): one file per
//     language and direction, named <lang>.input.txt and
//     <lang>.output.txt (docs/Coordinator.md section 9)
//   - the operator additions INPUT_DENY_WORDS / OUTPUT_DENY_WORDS
//
// Every language file that exists is always active. Filtering is
// therefore independent of the client/player locale, so switching the
// client language can never bypass a rule.
type moderation struct {
	input  map[string]struct{}
	output map[string]struct{}
	// Phrases (rules containing a space) are matched boundary-wise;
	// single words are matched per token.
	inputPhrases  []string
	outputPhrases []string
}

func (m *moderation) blockInput(text string) bool {
	return blockedWord(m.input, m.inputPhrases, text)
}

func (m *moderation) blockOutput(text string) bool {
	return blockedWord(m.output, m.outputPhrases, text)
}

// moderation returns the lazily loaded, shared filter state of the
// configuration. Loading happens once per Config instance and is safe
// for concurrent use by handler goroutines and queue workers.
func (c *Config) moderation() *moderation {
	c.modOnce.Do(func() {
		c.modValue = loadModeration(c.FilterDir, c.InputDenyWords, c.OutputDenyWords)
	})
	return c.modValue
}

// loadModeration builds the complete filter state. A missing filter
// directory is tolerated and yields empty file rules (only the operator
// additions remain active); empty and comment-only files are fine too.
// Files are matched by name: anything ending in .input.txt contributes
// to the input rules and anything ending in .output.txt to the output
// rules. The language prefix (en, de, ...) is informational — it
// decides nothing at runtime.
func loadModeration(dir string, inputWords, outputWords []string) *moderation {
	m := &moderation{
		input:  map[string]struct{}{},
		output: map[string]struct{}{},
	}
	if dir != "" {
		if entries, err := os.ReadDir(dir); err == nil {
			for _, e := range entries {
				if e.IsDir() {
					continue
				}
				name := e.Name()
				switch {
				case strings.HasSuffix(name, ".input.txt"):
					readFilterFile(filepath.Join(dir, name), m, true)
				case strings.HasSuffix(name, ".output.txt"):
					readFilterFile(filepath.Join(dir, name), m, false)
				}
			}
		}
	}
	for _, w := range inputWords {
		addFilterRule(w, m, true)
	}
	for _, w := range outputWords {
		addFilterRule(w, m, false)
	}
	return m
}

// readFilterFile adds every rule of one filter file. Lines starting
// with '#' are comments; blank lines and surrounding space are ignored.
// A broken file is skipped: the rules already collected stay effective,
// so a missing or unreadable file never weakens the filters below the
// rules that could be read.
func readFilterFile(path string, m *moderation, isInput bool) {
	data, err := os.ReadFile(path)
	if err != nil {
		return
	}
	for _, raw := range strings.Split(string(data), "\n") {
		if line := strings.TrimSpace(raw); line != "" && !strings.HasPrefix(line, "#") {
			addFilterRule(line, m, isInput)
		}
	}
}

// addFilterRule normalizes one rule (lower case) and adds it: single
// words go into the word set, phrases (with a space) into the phrase
// list for boundary-wise matching.
func addFilterRule(rule string, m *moderation, isInput bool) {
	rule = strings.ToLower(strings.TrimSpace(rule))
	if rule == "" {
		return
	}
	words := &m.output
	phrases := &m.outputPhrases
	if isInput {
		words = &m.input
		phrases = &m.inputPhrases
	}
	if strings.ContainsRune(rule, ' ') {
		*phrases = append(*phrases, rule)
		return
	}
	(*words)[rule] = struct{}{}
}

// blockedWord reports whether the text contains any rule as a whole
// word or (for phrases) as a boundary-delimited sequence. Matching is
// Unicode-aware (umlauts, accents) and case-insensitive; "evil" never
// matches "devil" or letters inside another word, only the word "evil".
func blockedWord(set map[string]struct{}, phrases []string, text string) bool {
	low := strings.ToLower(text)
	start := -1
	for i, r := range low {
		if unicode.IsLetter(r) || unicode.IsNumber(r) {
			if start < 0 {
				start = i
			}
			continue
		}
		if start >= 0 {
			if _, ok := set[low[start:i]]; ok {
				return true
			}
			start = -1
		}
	}
	if start >= 0 {
		if _, ok := set[low[start:]]; ok {
			return true
		}
	}
	for _, p := range phrases {
		if phraseMatches(low, p) {
			return true
		}
	}
	return false
}

// phraseMatches searches for a phrase that is delimited by non-word
// characters on both sides (start/end of text counts as a boundary).
func phraseMatches(text, phrase string) bool {
	idx := strings.Index(text, phrase)
	for idx >= 0 {
		if boundaryBefore(text, idx) && boundaryAfter(text, idx+len(phrase)) {
			return true
		}
		idx += len(phrase)
		if idx >= len(text) {
			break
		}
		next := strings.Index(text[idx:], phrase)
		if next < 0 {
			break
		}
		idx += next
	}
	return false
}

func boundaryBefore(s string, idx int) bool {
	if idx <= 0 {
		return true
	}
	r, _ := utf8.DecodeLastRuneInString(s[:idx])
	return !unicode.IsLetter(r) && !unicode.IsNumber(r)
}

func boundaryAfter(s string, idx int) bool {
	if idx >= len(s) {
		return true
	}
	r, _ := utf8.DecodeRuneInString(s[idx:])
	return !unicode.IsLetter(r) && !unicode.IsNumber(r)
}
