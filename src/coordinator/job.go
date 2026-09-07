package main

import (
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
	"time"
)

// Job statuses the coordinator persists. Only these values may appear
// in job files; everything else counts as a broken file.
const (
	jobPending    = "PENDING"
	jobProcessing = "PROCESSING"
	jobCompleted  = "COMPLETED"
	jobFailed     = "FAILED"
)

// Terminal result/error codes the coordinator reports to realms
// (docs/Coordinator.md sections 4, 7, 11, 12, 13; docs/ai_system.md
// section 25). The realm maps them to fixed local fallback reactions;
// none of them ever triggers a new KI request.
const (
	CodeQueued           = "QUEUED"
	CodeQueueFull        = "QUEUE_FULL"
	CodeInputTooLong     = "INPUT_TOO_LONG"
	CodeInputInvalid     = "INPUT_INVALID"
	CodeCooldown         = "COOLDOWN"
	CodeUnknownJobType   = "UNKNOWN_JOB_TYPE"
	CodeJobExists        = "JOB_EXISTS"
	CodeAIAvailable      = "AI_AVAILABLE"
	CodeAIUnavailable    = "AI_UNAVAILABLE"
	CodeTimeout          = "TIMEOUT"
	CodeContextTooLarge  = "CONTEXT_TOO_LARGE"
	CodeInvalidResponse  = "INVALID_RESPONSE"
	CodeValidationFailed = "VALIDATION_FAILED"
	CodeJobNotFound      = "JOB_NOT_FOUND"
)

// jobContext is the optional structured context a realm attaches to a
// job. It is copied verbatim into the Ollama prompt (subject to the
// context budget) and mirrors docs/ai_system.md section 6. The context
// may only contain information the realm decided to share; the
// coordinator does not add world state of its own.
type jobContext struct {
	Locale       string `json:"locale,omitempty"`
	SystemPrompt string `json:"system_prompt,omitempty"`
	World        string `json:"world,omitempty"`
	NPC          string `json:"npc,omitempty"`
	Situation    string `json:"situation,omitempty"`
	// AllowActionJSON requests a structured action answer (docs/
	// ai_system.md section 9). The coordinator validates it against
	// AllowedActionKeys and treats it strictly as a suggestion.
	AllowActionJSON bool `json:"allow_action_json,omitempty"`
}

// Job is one persistent coordinator job. Its file name is
// "<timestamp>_<type>-<id>.json" (docs/Coordinator.md section 14);
// the actual distributed data lives in the file, not in queue.json.
type Job struct {
	Version     int        `json:"version"`
	File        string     `json:"file,omitempty"`
	RealmID     string     `json:"realm_id"`
	JobID       string     `json:"job_id"`
	JobType     string     `json:"job_type"`
	CharacterID string     `json:"character_id,omitempty"`
	PlayerID    string     `json:"player_id,omitempty"`
	NPCID       string     `json:"npc_id,omitempty"`
	Text        string     `json:"text"`
	Context     jobContext `json:"context,omitempty"`
	Priority    int        `json:"priority"`
	CreatedAt   time.Time  `json:"created_at"`
	UpdatedAt   time.Time  `json:"updated_at"`
	Status      string     `json:"status"`
	Attempts    int        `json:"attempts"`
	MaxAttempts int        `json:"max_attempts"`
	Result      string     `json:"result,omitempty"`
	ErrorCode   string     `json:"error_code,omitempty"`
	ErrorMsg    string     `json:"error_msg,omitempty"`
}

var (
	jobTypeRe = regexp.MustCompile(`^[a-z][a-z0-9-_]*$`)
	jobIDRe   = regexp.MustCompile(`^[A-Za-z0-9_-]{1,128}$`)
)

// validJobType reports whether typ is one of the configured job types.
// The closed set keeps file names and priority derivation well-defined.
func validJobType(typ string, cfg *Config) bool {
	if !jobTypeRe.MatchString(typ) {
		return false
	}
	for _, p := range defaultPriorities {
		if p.Type == typ {
			return true
		}
	}
	return false
}

// validJobID reports whether the realm-provided job reference is
// usable in a file name (no path separators, no dots).
func validJobID(id string) bool {
	return jobIDRe.MatchString(id)
}

// statusValid reports whether a persisted status is one of the known
// values (used when loading job files during recovery).
func statusValid(s string) bool {
	switch s {
	case jobPending, jobProcessing, jobCompleted, jobFailed:
		return true
	}
	return false
}

// fileName renders the on-disk name of a job file.
func fileName(ts time.Time, typ, id string) string {
	return ts.Format("20060102T150405.000") + "_" + typ + "-" + id + ".json"
}

// parseFileName extracts timestamp, type and id from a job file name.
func parseFileName(name string) (time.Time, string, string, bool) {
	n := strings.TrimSuffix(name, ".json")
	us := strings.Index(n, "_")
	if us < 0 {
		return time.Time{}, "", "", false
	}
	ts, err := time.ParseInLocation("20060102T150405.000", n[:us], time.Local)
	if err != nil {
		return time.Time{}, "", "", false
	}
	rest := n[us+1:]
	dash := strings.Index(rest, "-")
	if dash < 0 {
		return time.Time{}, "", "", false
	}
	return ts, rest[:dash], rest[dash+1:], true
}

// key identifies a job for lookups within the coordinator scope:
// realm_id + the realm-provided job_id. The same realm-provided id can
// only ever exist once.
func (j *Job) key() string {
	return j.RealmID + "/" + j.JobID
}

// clone returns a deep copy for safe handover between goroutines.
func (j *Job) clone() *Job {
	out := *j
	out.Context = j.Context
	return &out
}

// NewJobFromPayload builds a pending Job from a realm submission. The
// payload field names mirror the coordinator's public API; unknown
// fields are ignored (the realm may send richer context later).
func NewJobFromPayload(in jobSubmission, cfg *Config, now time.Time) (*Job, error) {
	typ := strings.ToLower(strings.TrimSpace(in.JobType))
	if !validJobType(typ, cfg) {
		return nil, fmt.Errorf("unknown job type %q", in.JobType)
	}
	if !validJobID(in.JobID) {
		return nil, fmt.Errorf("invalid job id %q", in.JobID)
	}
	id := in.JobID
	// The same realm id pair must stay unique within the queue store;
	// this is enforced by the store at accept time (see store.Add).
	weight, ok := cfg.Priorities[typ]
	if !ok {
		weight = 0
	}
	return &Job{
		Version:     1,
		RealmID:     in.RealmID,
		JobID:       id,
		JobType:     typ,
		CharacterID: in.CharacterID,
		PlayerID:    in.PlayerID,
		NPCID:       in.NPCID,
		Text:        in.Text,
		Context:     in.Context,
		Priority:    weight,
		CreatedAt:   now,
		UpdatedAt:   now,
		Status:      jobPending,
		MaxAttempts: cfg.MaxAttempts,
	}, nil
}

// codeText maps a technical result code to a short German operating
// note for the realm (the realm decides its own fallback reaction).
func codeText(code string) string {
	switch code {
	case CodeQueueFull:
		return "Koordinator-Warteschlange voll"
	case CodeInputTooLong:
		return "Eingabe zu lang"
	case CodeInputInvalid:
		return "Eingabe gegen Inhaltsregeln verstoßen"
	case CodeCooldown:
		return "Cooldown aktiv"
	case CodeAIUnavailable:
		return "KI-Dienst nicht verfügbar"
	case CodeTimeout:
		return "Zeitüberschreitung"
	case CodeContextTooLarge:
		return "Kontextbudget überschritten"
	case CodeInvalidResponse:
		return "Ungültige KI-Antwort"
	case CodeValidationFailed:
		return "KI-Antwort nicht regelkonform"
	case CodeJobNotFound:
		return "Job nicht gefunden"
	case CodeUnknownJobType:
		return "Unbekannter Jobtyp"
	default:
		return code
	}
}

// cooldownKey is the identity used for the per-player cooldown: the
// realm-provided player_id, falling back to the character_id. Both are
// coordinator-internal operational IDs, never account data. Jobs
// without either skip the cooldown check (system-driven jobs).
func (j *Job) cooldownKey() string {
	if j.PlayerID != "" {
		return j.PlayerID
	}
	return j.CharacterID
}

// encode renders the job to the persisted JSON form. The version is
// normalized so that older/hand-written files without an explicit
// version still decode on reload.
func (j *Job) encode() ([]byte, error) {
	if j.Version == 0 {
		j.Version = 1
	}
	return json.MarshalIndent(j, "", "  ")
}

// decodeJob parses a job file. Missing/invalid status rejects the file.
func decodeJob(b []byte) (*Job, error) {
	var j Job
	if err := json.Unmarshal(b, &j); err != nil {
		return nil, err
	}
	if j.Version != 1 || !statusValid(j.Status) || j.RealmID == "" || j.JobID == "" {
		return nil, fmt.Errorf("invalid job document")
	}
	return &j, nil
}
