package main

import (
	"os"
	"path/filepath"
	"testing"
)

func writeFilterDir(t *testing.T, files map[string]string) string {
	t.Helper()
	dir := t.TempDir()
	for name, content := range files {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return dir
}

func TestLoadModerationMultiLanguageCombined(t *testing.T) {
	dir := writeFilterDir(t, map[string]string{
		"en.input.txt":  "# english master\nziplockrule\n",
		"de.input.txt":  "schattenwort\n",
		"de.output.txt": "nachhallgeheim\n",
		"en.output.txt": "",
	})
	m := loadModeration(dir, nil, nil)
	// Words from both language files are active at the same time,
	// independent of any client/player locale.
	for _, text := range []string{"the ziplockrule is active", "ich sah das schattenwort"} {
		if !m.blockInput(text) {
			t.Errorf("input %q must be blocked", text)
		}
	}
	if !m.blockOutput("nachhallgeheim verrät es") {
		t.Error("de.output rule must block output")
	}
}

func TestModerationInputOutputSeparated(t *testing.T) {
	dir := writeFilterDir(t, map[string]string{
		"en.input.txt":  "strenggeheim\n",
		"en.output.txt": "antwortleak\n",
	})
	m := loadModeration(dir, nil, nil)
	if m.blockOutput("das ist strenggeheim") {
		t.Error("input rule must not apply to output")
	}
	if m.blockInput("antwortleak") {
		t.Error("output rule must not apply to input")
	}
	if !m.blockInput("strenggeheim") || !m.blockOutput("antwortleak") {
		t.Error("simple same-side rules must still block")
	}
}

func TestModerationEnvAdditionsMerged(t *testing.T) {
	m := loadModeration("", []string{"operatorword1"}, []string{"operatorword2"})
	if !m.blockInput("operatorword1 here") {
		t.Error("env input addition must block")
	}
	if !m.blockOutput("operatorword2") {
		t.Error("env output addition must block")
	}
	if m.blockInput("operatorword2") {
		t.Error("cross-side env word must not block")
	}
}

func TestModerationMissingDirIsEmpty(t *testing.T) {
	m := loadModeration(filepath.Join(t.TempDir(), "does-not-exist"), nil, nil)
	if m.blockInput("irgendwas") || m.blockOutput("irgendwas") {
		t.Error("missing filter dir must yield empty rules")
	}
}

func TestModerationEmptyAndCommentOnlyFiles(t *testing.T) {
	dir := writeFilterDir(t, map[string]string{
		"en.input.txt":  "# nur kommentare\n\n",
		"en.output.txt": "",
	})
	m := loadModeration(dir, []string{"envword"}, nil)
	if !m.blockInput("envword") {
		t.Error("env additions stay active alongside empty files")
	}
	if m.blockInput("hallo") || m.blockOutput("hallo") {
		t.Error("no rule may match arbitrary text")
	}
}

func TestModerationCaseInsensitiveAndBoundaries(t *testing.T) {
	m := loadModeration("", []string{"Evil", "straße"}, nil)
	for _, hit := range []string{"Evil plan", "evil", "die Straße"} {
		if !m.blockInput(hit) {
			t.Errorf("%q must be blocked", hit)
		}
	}
	// No ss/ß folding and no substring matching: "strasse"/"straße"
	// stay distinct, and word rules never hit a letter inside another word.
	for _, miss := range []string{"devil", "evilness", "strasse", "plain text"} {
		if m.blockInput(miss) {
			t.Errorf("%q must NOT be blocked", miss)
		}
	}
}

func TestModerationPhrases(t *testing.T) {
	c := &Config{InputDenyWords: []string{"viele worte"}, OutputDenyWords: []string{"geheime antwort sätze"}}
	dir := writeFilterDir(t, map[string]string{
		"en.input.txt":  "vertraulich worte\n",
		"en.output.txt": "",
	})
	c.FilterDir = dir
	mod := c.moderation()
	if !mod.blockInput("Er sagte viele Worte dazu.") {
		t.Error("phrase with spaces must match across words")
	}
	if mod.blockInput("vielviele worte") {
		t.Error("phrase must be boundary-delimited")
	}
	if !mod.blockInput("vertraulich worte zwischen dir und mir") {
		t.Error("file phrase must match")
	}
	if mod.blockOutput("heute sagst du viele Worte.") {
		t.Error("input phrase must not apply to output")
	}
}

func TestConfigFilterDirWiring(t *testing.T) {
	dir := writeFilterDir(t, map[string]string{
		"en.input.txt":  "dateiwort\n",
		"de.output.txt": "ausgabewort\n",
	})
	cfg := loadEnvString(t, minimalEnv+"COORDINATOR_FILTER_DIR="+dir+"\nINPUT_DENY_WORDS=envwort\n")
	if cfg.FilterDir != filepath.Clean(dir) {
		t.Errorf("FilterDir = %q", cfg.FilterDir)
	}
	if !cfg.moderation().blockInput("dateiwort") || !cfg.moderation().blockInput("envwort") {
		t.Error("config moderation must merge file and env rules")
	}
	if !cfg.moderation().blockOutput("ausgabewort") {
		t.Error("config moderation must load output file rules")
	}
	if cfg.moderation() != cfg.moderation() {
		t.Error("moderation must be loaded exactly once")
	}
}

func TestConfigFilterDirDefault(t *testing.T) {
	cfg := loadEnvString(t, minimalEnv)
	if !filepath.IsAbs(cfg.FilterDir) || cfg.FilterDir == "" {
		t.Errorf("FilterDir default wrong: %q", cfg.FilterDir)
	}
}

func TestValidatorUsesModeration(t *testing.T) {
	dir := writeFilterDir(t, map[string]string{
		"en.input.txt":  "strengwichtig\n",
		"de.output.txt": "heimausgabe\n",
	})
	cfg := loadEnvString(t, minimalEnv+"COORDINATOR_FILTER_DIR="+dir+"\n")
	v := newValidator(cfg)
	if rv := v.inputValidation(&Job{Text: "strengwichtig ist klar"}); rv.OK || rv.Reason != CodeInputInvalid {
		t.Errorf("input validation: %+v", rv)
	}
	if rv := v.outputValidation(&Job{}, "heimausgabe in der antwort"); rv.OK || rv.Reason != CodeValidationFailed {
		t.Errorf("output validation: %+v", rv)
	}
}
