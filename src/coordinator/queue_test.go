package main

import (
	"context"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

// fakeOllama simulates the Ollama backend for queue tests.
type fakeOllama struct {
	mu      sync.Mutex
	results []string
	class   string
	calls   int
	pingOK  bool
}

func (f *fakeOllama) chat(ctx context.Context, msgs []chatMessage) (string, string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls++
	if f.class != "" {
		return "", f.class
	}
	if len(f.results) > 0 {
		r := f.results[0]
		f.results = f.results[1:]
		return r, ""
	}
	return "Hallo Welt!", ""
}

func (f *fakeOllama) ping(ctx context.Context) bool { return f.pingOK }

func newQueue(t *testing.T, cfgExtra string, ollama ollamaGate) (*queueService, *fileStore) {
	t.Helper()
	cfg := loadEnvString(t, minimalEnv+cfgExtra)
	cfg.DataDir = filepath.Join(t.TempDir(), "data")
	store := newFileStore(cfg.DataDir)
	if err := store.Init(); err != nil {
		t.Fatal(err)
	}
	return newQueueService(cfg, store, ollama, newRealmClient(cfg)), store
}

// mustJob writes a pending job directly into the store. MaxAttempts is
// taken from the queue config so the correction budgets in tests behave
// exactly like production.
func mustJob(t *testing.T, q *queueService, store *fileStore, realmID, jt, id string, now time.Time) *Job {
	t.Helper()
	j := &Job{
		RealmID:     realmID,
		JobType:     jt,
		JobID:       id,
		Text:        "hallo",
		Status:      jobPending,
		Priority:    90,
		MaxAttempts: q.cfg.MaxAttempts,
		Context:     jobContext{},
		CreatedAt:   now,
		UpdatedAt:   now,
	}
	if _, err := store.Add(j, now); err != nil {
		t.Fatal(err)
	}
	return j
}

func TestNextPendingPriorityOrder(t *testing.T) {
	q, store := newQueue(t, "", &fakeOllama{})
	now := time.Now()
	jRaid := mustJob(t, q, store, "de1", "raidboss", "r1", now)
	jCraft := mustJob(t, q, store, "de1", "craft", "c1", now.Add(-time.Minute))
	jW1 := mustJob(t, q, store, "de1", "world", "w1", now)
	_ = jCraft
	jRaid.Priority = 100
	jW1.Priority = 10
	if err := store.Save(jRaid); err != nil {
		t.Fatal(err)
	}
	if err := store.Save(jW1); err != nil {
		t.Fatal(err)
	}

	order := []string{}
	for i := 0; i < 3; i++ {
		j := q.nextPending()
		if j == nil {
			t.Fatal("expected a pending job")
		}
		order = append(order, j.JobID)
	}
	if order[0] != "r1" || order[2] != "w1" {
		t.Errorf("priority order wrong: %v", order)
	}
}

func TestNextPendingArrivalOrderTie(t *testing.T) {
	q, store := newQueue(t, "", &fakeOllama{})
	now := time.Now()
	mustJob(t, q, store, "de1", "player", "a", now)
	mustJob(t, q, store, "de1", "player", "b", now.Add(1*time.Second))
	first := q.nextPending()
	if first == nil {
		t.Fatal("no pending job")
	}
	if first.JobID != "a" {
		t.Errorf("arrival order: first = %s", first.JobID)
	}
}

func TestAcceptDuplicateRejected(t *testing.T) {
	q, store := newQueue(t, "", &fakeOllama{})
	now := time.Now()
	j := mustJob(t, q, store, "de1", "player", "dup", now)
	if err := q.accept(j, now); err != ErrDuplicate {
		t.Fatalf("accept duplicate = %v, want ErrDuplicate", err)
	}
}

func TestAcceptCooldown(t *testing.T) {
	q, _ := newQueue(t, "PLAYER_COOLDOWN_SECONDS=60\n", &fakeOllama{})
	now := time.Now()
	good := &Job{RealmID: "de1", JobType: "player", JobID: "p1", Text: "a",
		Status: jobPending, MaxAttempts: 5, Priority: 90, PlayerID: "pollo", CreatedAt: now, UpdatedAt: now}
	if err := q.accept(good, now); err != nil {
		t.Fatal(err)
	}
	again := &Job{RealmID: "de1", JobType: "player", JobID: "p2", Text: "b",
		Status: jobPending, MaxAttempts: 5, Priority: 90, PlayerID: "pollo", CreatedAt: now, UpdatedAt: now}
	if err := q.accept(again, now); err == nil {
		t.Fatal("same player within cooldown must be rejected")
	} else if _, ok := err.(*cooldownError); !ok {
		t.Fatalf("expected cooldownError, got %v", err)
	}
	other := &Job{RealmID: "de1", JobType: "player", JobID: "p3", Text: "c",
		Status: jobPending, MaxAttempts: 5, Priority: 90, PlayerID: "anderer", CreatedAt: now, UpdatedAt: now}
	if err := q.accept(other, now); err != nil {
		t.Fatalf("different player must pass: %v", err)
	}
}

func TestRequeueStaleResetsProcessing(t *testing.T) {
	q, store := newQueue(t, "", &fakeOllama{})
	now := time.Now()
	j := mustJob(t, q, store, "de1", "player", "stale", now)
	j.Status = jobProcessing
	if err := store.Save(j); err != nil {
		t.Fatal(err)
	}
	q.requeueStale()
	jobs, _, _ := store.LoadPending()
	for _, p := range jobs {
		if p.Status != jobPending {
			t.Errorf("stale job still %s", p.Status)
		}
	}
}

func TestProcessCompletesJob(t *testing.T) {
	ollama := &fakeOllama{results: []string{"Das ist die Antwort."}}
	q, store := newQueue(t, "", ollama)
	now := time.Now()
	mustJob(t, q, store, "de1", "player", "c1", now)
	// nextPending marks it PROCESSING before process() runs (same as run())
	j2 := q.nextPending()
	if j2 == nil {
		t.Fatal("no pending job")
	}
	q.process(j2)
	if j2.Status != jobCompleted {
		t.Fatalf("status = %s", j2.Status)
	}
	if j2.Result != "Das ist die Antwort." {
		t.Errorf("result = %q", j2.Result)
	}
	if q.completed != 1 {
		t.Errorf("completed = %d", q.completed)
	}
	// done job must no longer be pending
	jobs, _, _ := store.LoadPending()
	if len(jobs) != 0 {
		t.Error("completed job still pending")
	}
}

func TestProcessAIUnavailableFails(t *testing.T) {
	ollama := &fakeOllama{class: CodeAIUnavailable}
	q, store := newQueue(t, "", ollama)
	mustJob(t, q, store, "de1", "player", "f1", time.Now())
	j2 := q.nextPending()
	q.process(j2)
	if j2.Status != jobFailed || j2.ErrorCode != CodeAIUnavailable {
		t.Errorf("status/code = %s/%s", j2.Status, j2.ErrorCode)
	}
}

func TestProcessValidationRetryThenReturn(t *testing.T) {
	ollama := &fakeOllama{}
	q, store := newQueue(t, "OUTPUT_DENY_WORDS=verboten\nMAX_CORRECTION_ATTEMPTS=2\n", ollama)
	ollama.results = []string{"Das ist verboten.", "Korrekte Antwort."}
	mustJob(t, q, store, "de1", "player", "v1", time.Now())
	j2 := q.nextPending()
	q.process(j2)
	if j2.Status != jobCompleted {
		t.Fatalf("status = %s, err %s", j2.Status, j2.ErrorCode)
	}
	if ollama.calls != 2 {
		t.Errorf("ollama calls = %d, want 2", ollama.calls)
	}
}

func TestProcessValidationExhaustsFails(t *testing.T) {
	ollama := &fakeOllama{results: []string{"verboten", "verboten", "verboten"}}
	q, store := newQueue(t, "OUTPUT_DENY_WORDS=verboten\nMAX_CORRECTION_ATTEMPTS=2\n", ollama)
	mustJob(t, q, store, "de1", "player", "v2", time.Now())
	j2 := q.nextPending()
	q.process(j2)
	if j2.Status != jobFailed || j2.ErrorCode != CodeValidationFailed {
		t.Errorf("status/code = %s/%s", j2.Status, j2.ErrorCode)
	}
	if ollama.calls != 2 {
		t.Errorf("ollama calls = %d, want 2 (bounded by MaxAttempts)", ollama.calls)
	}
}

func TestRunDispatchProcessesQueuedJob(t *testing.T) {
	ollama := &fakeOllama{results: []string{"Antwort."}}
	q, store := newQueue(t, "", ollama)
	now := time.Now()
	mustJob(t, q, store, "de1", "player", "d1", now)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { q.run(ctx); close(done) }()
	deadline := time.Now().Add(3 * time.Second)
	for q.completed < 1 && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	cancel()
	<-done
	if q.completed != 1 {
		t.Fatalf("completed = %d", q.completed)
	}
}

func TestMetrics(t *testing.T) {
	q, store := newQueue(t, "", &fakeOllama{})
	now := time.Now()
	mustJob(t, q, store, "de1", "player", "m1", now)
	m := q.metrics()
	p := m["queue"].(map[string]int)
	if p["pending"] != 1 {
		t.Errorf("metrics pending = %d", p["pending"])
	}
	if m["workers"] != 2 {
		t.Errorf("workers = %v", m["workers"])
	}
}
