package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

// fileStore implements the persistent, file-based coordinator queue
// (docs/Coordinator.md sections 14–18). Design:
//
//   - Every accepted job is ONE file: queue/jobs/<timestamp>_<type>-<id>.json.
//     A broken write can therefore never corrupt the whole queue.
//   - queue.json only holds the current order of pending job files; it
//     can be regenerated from the job files (Recover).
//   - All writes are atomic (temp file in the project area + fsync +
//     rename). System /tmp is never used.
//   - Terminal jobs move to queue/done/ (still queryable); broken files
//     move to queue/quarantine/ instead of being force-deleted.
//
// DataDir is the only configurable root; everything lives below it.
type fileStore struct {
	mu   sync.Mutex
	base string
}

var (
	ErrDuplicate = errors.New("duplicate job already queued")
	ErrNotFound  = errors.New("job not found")
)

func newFileStore(base string) *fileStore {
	return &fileStore{base: base}
}

func (st *fileStore) queueDir() string { return filepath.Join(st.base, "queue") }
func (st *fileStore) jobsDir() string  { return filepath.Join(st.base, "queue", "jobs") }
func (st *fileStore) doneDir() string  { return filepath.Join(st.base, "queue", "done") }
func (st *fileStore) quarantineDir() string {
	return filepath.Join(st.base, "queue", "quarantine")
}

// Init ensures the directory layout exists and recovers a usable
// queue.json (sections 16/17). It does not touch any existing valid
// file: recovery only kicks in when queue.json is missing.
func (st *fileStore) Init() error {
	for _, d := range []string{st.jobsDir(), st.doneDir(), st.quarantineDir()} {
		if err := os.MkdirAll(d, 0o750); err != nil {
			return fmt.Errorf("create %s: %w", d, err)
		}
	}
	if !st.queueFileExists() {
		if err := st.Rebuild(); err != nil {
			return fmt.Errorf("recover queue: %w", err)
		}
		return nil
	}
	// Validate references: entries pointing at missing files do not
	// block the queue (section 18); workers skip and quarantine them.
	return nil
}

func (st *fileStore) queueFileExists() bool {
	_, err := os.Stat(st.queueJSONPath())
	return err == nil
}

func (st *fileStore) queueJSONPath() string { return filepath.Join(st.queueDir(), "queue.json") }

// queueJSON is the persisted order list.
type queueJSON struct {
	Version int      `json:"version"`
	Jobs    []string `json:"jobs"`
}

// Rebuild reconstructs queue.json from the job files (section 17).
// The order is by timestamp in the file name (arrival order); the
// worker applies priority weighting on top.
func (st *fileStore) Rebuild() error {
	names, err := st.jobFileNames()
	if err != nil {
		return err
	}
	sort.Strings(names)
	return st.writeQueue(names)
}

// jobFileNames lists all non-terminal job files.
func (st *fileStore) jobFileNames() ([]string, error) {
	entries, err := os.ReadDir(st.jobsDir())
	if err != nil {
		return nil, err
	}
	var names []string
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".json") {
			continue
		}
		names = append(names, e.Name())
	}
	return names, nil
}

// readQueue returns the pending job order from queue.json. A missing
// queue.json first triggers a rebuild. References are returned
// verbatim; missing files are handled by the caller (section 18).
func (st *fileStore) readQueue() ([]string, error) {
	if !st.queueFileExists() {
		if err := st.Rebuild(); err != nil {
			return nil, err
		}
	}
	raw, err := os.ReadFile(st.queueJSONPath())
	if err != nil {
		return nil, err
	}
	var q queueJSON
	if err := json.Unmarshal(raw, &q); err != nil || q.Version != 1 {
		// A corrupt queue.json must not kill the queue: rebuild.
		if err := st.Rebuild(); err != nil {
			return nil, err
		}
		return st.readQueue()
	}
	return q.Jobs, nil
}

func (st *fileStore) writeQueue(names []string) error {
	q := queueJSON{Version: 1, Jobs: names}
	raw, err := json.MarshalIndent(q, "", "  ")
	if err != nil {
		return err
	}
	return st.atomicWrite(st.queueJSONPath(), raw)
}

// Add persists a new pending job (sections 14–16). The realm-provided
// (realm_id, job_id) pair may exist only once; the file name is derived
// from the arrival timestamp, job type and id, with a numeric
// disambiguator for the (unlikely) same-name collision.
func (st *fileStore) Add(j *Job, now time.Time) (string, error) {
	st.mu.Lock()
	defer st.mu.Unlock()

	if _, ok, err := st.findLocked(j.RealmID, j.JobID); err != nil {
		return "", err
	} else if ok {
		return "", ErrDuplicate
	}

	name := fileName(now, j.JobType, j.JobID)
	for i := 2; ; i++ {
		if _, err := os.Stat(filepath.Join(st.jobsDir(), name)); os.IsNotExist(err) {
			break
		}
		name = fileName(now, j.JobType, fmt.Sprintf("%s-%d", j.JobID, i))
	}
	j.File = name
	j.CreatedAt = now
	j.UpdatedAt = now
	if err := st.saveLocked(j); err != nil {
		return "", err
	}
	names, err := st.readQueue()
	if err != nil {
		return "", err
	}
	names = append(names, name)
	if err := st.writeQueue(names); err != nil {
		return "", err
	}
	return name, nil
}

// Save atomically updates the file of an existing job (status changes,
// result, attempts, ...).
func (st *fileStore) Save(j *Job) error {
	st.mu.Lock()
	defer st.mu.Unlock()
	return st.saveLocked(j)
}

func (st *fileStore) saveLocked(j *Job) error {
	if j.File == "" {
		return fmt.Errorf("job has no file name")
	}
	raw, err := j.encode()
	if err != nil {
		return err
	}
	return st.atomicWrite(filepath.Join(st.jobsDir(), j.File), raw)
}

// saveDone writes a terminal job into the done area (kept for §27
// queries/support) and removes it from the pending queue.json.
func (st *fileStore) saveDone(j *Job) error {
	raw, err := j.encode()
	if err != nil {
		return err
	}
	dst := filepath.Join(st.doneDir(), j.File)
	if err := st.atomicWrite(dst, raw); err != nil {
		return err
	}
	if err := st.removePending(j.File); err != nil {
		return err
	}
	return os.Remove(filepath.Join(st.jobsDir(), j.File))
}

// removePending drops one job file name from queue.json.
func (st *fileStore) removePending(name string) error {
	names, err := st.readQueue()
	if err != nil {
		return err
	}
	out := names[:0]
	for _, n := range names {
		if n != name {
			out = append(out, n)
		}
	}
	return st.writeQueue(out)
}

// Quarantine moves a broken/missing job file aside (section 18) and
// drops it from the queue. The original content survives for diagnosis.
func (st *fileStore) Quarantine(name string) error {
	st.mu.Lock()
	defer st.mu.Unlock()
	if err := st.removePending(name); err != nil {
		return err
	}
	src := filepath.Join(st.jobsDir(), name)
	if _, err := os.Stat(src); os.IsNotExist(err) {
		return nil // already gone
	}
	return os.Rename(src, filepath.Join(st.quarantineDir(), name))
}

// LoadPending returns every pending job (statuses PENDING or PROCESSING
// after recovery) plus the list of broken names that were skipped so
// the workers can report them to the responsible realm (section 18/24).
func (st *fileStore) LoadPending() ([]*Job, []string, error) {
	names, err := st.readQueue()
	if err != nil {
		return nil, nil, err
	}
	st.mu.Lock()
	defer st.mu.Unlock()
	var jobs []*Job
	var broken []string
	for _, n := range names {
		raw, err := os.ReadFile(filepath.Join(st.jobsDir(), n))
		if err != nil || !json.Valid(raw) {
			broken = append(broken, n)
			continue
		}
		j, err := decodeJob(raw)
		if err != nil {
			broken = append(broken, n)
			continue
		}
		jobs = append(jobs, j)
	}
	return jobs, broken, nil
}

// findLocked locates a job by (realm_id, job_id) in the pending AND
// done area. If a pending job with the same key exists, the second
// submission is a duplicate (Add rejects it). Done jobs answer §25/26
// "existiert Job <ID> noch?" with their terminal status.
func (st *fileStore) findLocked(realmID, jobID string) (*Job, bool, error) {
	for _, dir := range []string{st.jobsDir(), st.doneDir()} {
		entries, err := os.ReadDir(dir)
		if err != nil {
			return nil, false, err
		}
		for _, e := range entries {
			if e.IsDir() || !strings.HasSuffix(e.Name(), ".json") {
				continue
			}
			raw, err := os.ReadFile(filepath.Join(dir, e.Name()))
			if err != nil {
				continue
			}
			j, err := decodeJob(raw)
			if err != nil {
				continue
			}
			if j.RealmID == realmID && j.JobID == jobID {
				return j, true, nil
			}
		}
	}
	return nil, false, nil
}

// GetByKey answers §25/§26: whether a job still exists inside the
// coordinator's own scope and, if so, its current (possibly terminal)
// status including the result.
func (st *fileStore) GetByKey(realmID, jobID string) (*Job, bool, error) {
	st.mu.Lock()
	defer st.mu.Unlock()
	return st.findLocked(realmID, jobID)
}

// Stats reports the current queue inventory for /status.
func (st *fileStore) Stats() (pending, done, quarantine int, err error) {
	st.mu.Lock()
	defer st.mu.Unlock()
	names, err := st.readQueue()
	if err != nil {
		return 0, 0, 0, err
	}
	pending = len(names)
	done = countJSON(st.doneDir())
	quarantine = countJSON(st.quarantineDir())
	return pending, done, quarantine, nil
}

func countJSON(dir string) int {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return 0
	}
	n := 0
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".json") {
			n++
		}
	}
	return n
}

// atomicWrite implements the safe-write protocol (section 16): the new
// version is fully written as a temp file in the same (project-internal)
// directory, synced, then atomically renamed over the previous version.
func (st *fileStore) atomicWrite(path string, data []byte) error {
	dir := filepath.Dir(path)
	tmp, err := os.CreateTemp(dir, ".tmp-*")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName)
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Rename(tmpName, path); err != nil {
		return err
	}
	if d, err := os.Open(dir); err == nil {
		_ = d.Sync()
		_ = d.Close()
	}
	return nil
}
