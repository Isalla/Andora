package main

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func newTestStore(t *testing.T) *fileStore {
	t.Helper()
	store := newFileStore(filepath.Join(t.TempDir(), "data"))
	if err := store.Init(); err != nil {
		t.Fatal(err)
	}
	return store
}

func TestStoreInitCreatesDirs(t *testing.T) {
	dir := t.TempDir()
	store := newFileStore(filepath.Join(dir, "q"))
	if err := store.Init(); err != nil {
		t.Fatal(err)
	}
	for _, sub := range []string{
		"queue/jobs", "queue/done", "queue/quarantine",
	} {
		if _, err := os.Stat(filepath.Join(dir, "q", sub)); os.IsNotExist(err) {
			t.Errorf("missing directory %s", sub)
		}
	}
}

func TestStoreAddAndRead(t *testing.T) {
	store := newTestStore(t)
	j := &Job{
		RealmID:     "de1",
		JobType:     "player",
		JobID:       "j1",
		Text:        "hallo",
		Status:      jobPending,
		MaxAttempts: 5,
		CreatedAt:   time.Now(),
		UpdatedAt:   time.Now(),
	}
	if _, err := store.Add(j, time.Now()); err != nil {
		t.Fatal(err)
	}
	if j.File == "" {
		t.Fatal("Add must set j.File")
	}
	jobs, _, err := store.LoadPending()
	if err != nil {
		t.Fatal(err)
	}
	if len(jobs) != 1 {
		t.Fatalf("loaded %d jobs, want 1", len(jobs))
	}
	if jobs[0].JobID != "j1" {
		t.Errorf("job = %+v", jobs[0])
	}
}

func TestStoreDuplicateRejects(t *testing.T) {
	store := newTestStore(t)
	j1 := &Job{RealmID: "de1", JobType: "player", JobID: "j1", Status: jobPending, MaxAttempts: 5, CreatedAt: time.Now(), UpdatedAt: time.Now()}
	j2 := &Job{RealmID: "de1", JobType: "player", JobID: "j1", Status: jobPending, MaxAttempts: 5, CreatedAt: time.Now(), UpdatedAt: time.Now()}
	if _, err := store.Add(j1, time.Now()); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Add(j2, time.Now()); err != ErrDuplicate {
		t.Fatalf("second Add must return ErrDuplicate, got %v", err)
	}
}

func TestStoreFindLockedNotFound(t *testing.T) {
	store := newTestStore(t)
	j, exists, err := store.GetByKey("de1", "nobody")
	if err != nil {
		t.Fatal(err)
	}
	if exists || j != nil {
		t.Error("unknown job must not be found")
	}
}

func TestStoreSaveDone(t *testing.T) {
	store := newTestStore(t)
	j := &Job{RealmID: "de1", JobType: "player", JobID: "d1", Status: jobPending, MaxAttempts: 5, CreatedAt: time.Now(), UpdatedAt: time.Now()}
	if _, err := store.Add(j, time.Now()); err != nil {
		t.Fatal(err)
	}
	j.Status = jobCompleted
	j.Result = "answer"
	if err := store.saveDone(j); err != nil {
		t.Fatal(err)
	}
	jobs, _, _ := store.LoadPending()
	if len(jobs) != 0 {
		t.Errorf("done job still pending: %d", len(jobs))
	}
}

func TestStoreQuarantine(t *testing.T) {
	store := newTestStore(t)
	j := &Job{RealmID: "de1", JobType: "player", JobID: "q1", Status: jobPending, MaxAttempts: 5, CreatedAt: time.Now(), UpdatedAt: time.Now()}
	if _, err := store.Add(j, time.Now()); err != nil {
		t.Fatal(err)
	}
	if err := store.Quarantine(j.File); err != nil {
		t.Fatal(err)
	}
	jobs, _, _ := store.LoadPending()
	if len(jobs) != 0 {
		t.Errorf("quarantined job still pending")
	}
}

func TestStoreRemovePending(t *testing.T) {
	store := newTestStore(t)
	j := &Job{RealmID: "de1", JobType: "player", JobID: "r1", Status: jobPending, MaxAttempts: 5, CreatedAt: time.Now(), UpdatedAt: time.Now()}
	if _, err := store.Add(j, time.Now()); err != nil {
		t.Fatal(err)
	}
	if err := store.removePending(j.File); err != nil {
		t.Fatal(err)
	}
	jobs, _, _ := store.LoadPending()
	if len(jobs) != 0 {
		t.Error("removed job still pending")
	}
}

func TestStoreRebuild(t *testing.T) {
	store := newTestStore(t)
	j := &Job{RealmID: "de1", JobType: "player", JobID: "rb1", Status: jobPending, MaxAttempts: 5, CreatedAt: time.Now(), UpdatedAt: time.Now()}
	if _, err := store.Add(j, time.Now()); err != nil {
		t.Fatal(err)
	}
	queuePath := filepath.Join(store.base, "queue", "queue.json")
	data, _ := os.ReadFile(queuePath)
	if err := os.WriteFile(queuePath, data[:len(data)/2], 0o644); err != nil {
		t.Fatal(err)
	}
	if err := store.Rebuild(); err != nil {
		t.Fatal(err)
	}
	jobs, _, _ := store.LoadPending()
	if len(jobs) != 1 {
		t.Errorf("rebuild lost job: %d", len(jobs))
	}
}

func TestStoreJobFileNames(t *testing.T) {
	ts := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	fn1 := fileName(ts, "player", "a")
	fn2 := fileName(ts, "player", "a")
	if fn1 != fn2 {
		t.Errorf("same timestamp must produce same file name, got %v", [2]string{fn1, fn2})
	}
}
