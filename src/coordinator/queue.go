package main

import (
	"context"
	"log"
	"sort"
	"sync"
	"time"
)

// cooldownError reports a per-player cooldown rejection (§6) with the
// remaining wait time (Retry-After).
type cooldownError struct{ retry time.Duration }

func (e *cooldownError) Error() string {
	return "player cooldown active"
}

// queueService is the runtime core of the coordinator: the persistent
// queue is file-backed; this type schedules, executes and reports jobs
// against Ollama. It holds no database and no game state.
type queueService struct {
	cfg    *Config
	store  *fileStore
	ollama ollamaGate
	realms *realmClient
	start  time.Time

	mu        sync.Mutex
	cooldowns map[string]time.Time

	sem    chan struct{}
	wg     sync.WaitGroup
	wakeCh chan struct{}

	active    int
	completed int
	failed    int
	lastOK    bool
	lastOKAt  time.Time
}

// ollamaGate is the Ollama surface the queue depends on (real client in
// production, fake in tests).
type ollamaGate interface {
	chat(ctx context.Context, messages []chatMessage) (string, string)
	ping(ctx context.Context) bool
}

func newQueueService(cfg *Config, store *fileStore, ollama ollamaGate, realms *realmClient) *queueService {
	return &queueService{
		cfg:       cfg,
		store:     store,
		ollama:    ollama,
		realms:    realms,
		start:     time.Now(),
		cooldowns: map[string]time.Time{},
		sem:       make(chan struct{}, cfg.Workers),
		wakeCh:    make(chan struct{}, 1),
	}
}

// run is the dispatch loop. It stops picking new jobs when ctx is
// cancelled, lets in-flight jobs finish (§19) with a bounded grace, and
// returns. The coordinator then persists a consistent queue.json.
func (q *queueService) run(ctx context.Context) {
	if q.cfg.Workers < 1 {
		log.Panic("coordinator: COORDINATOR_WORKERS must be >= 1")
	}
	for {
		if ctx.Err() != nil {
			break
		}
		select {
		case q.sem <- struct{}{}:
		case <-ctx.Done():
			continue
		}
		j := q.nextPending()
		if j == nil {
			<-q.sem // released room, nothing to do right now
			select {
			case <-ctx.Done():
			case <-q.wakeCh:
			case <-time.After(500 * time.Millisecond):
			}
			continue
		}
		q.wg.Add(1)
		go func(j *Job) {
			defer func() {
				<-q.sem
				q.wg.Done()
			}()
			q.process(j)
		}(j)
	}
	// Graceful shutdown: wait for in-flight jobs, bounded by the
	// per-request Ollama timeout (a hung call must not block exit).
	done := make(chan struct{})
	go func() {
		q.wg.Wait()
		close(done)
	}()
	grace := 30 * time.Second
	if b := q.cfg.OllamaTimeout * time.Duration(q.cfg.MaxAttempts); b < grace {
		grace = b
	}
	select {
	case <-done:
	case <-time.After(grace):
		log.Print("coordinator: shutdown grace exceeded")
	}
}

// requeueStale resets jobs that were left PROCESSING by an unclean
// shutdown back to PENDING. Clean shutdown finishes its current job, so
// on every restart PROCESSING entries are stale by definition.
func (q *queueService) requeueStale() {
	jobs, _, err := q.store.LoadPending()
	if err != nil {
		log.Printf("coordinator: requeue scan: %v", err)
		return
	}
	for _, j := range jobs {
		if j.Status != jobProcessing {
			continue
		}
		j.Status = jobPending
		j.UpdatedAt = time.Now()
		if err := q.store.Save(j); err != nil {
			log.Printf("coordinator: requeue %s: %v", j.File, err)
		} else {
			log.Printf("coordinator: requeued stale job %s", j.File)
		}
	}
}

// wake nudges the dispatcher (called after a successful accept).
func (q *queueService) wake() {
	select {
	case q.wakeCh <- struct{}{}:
	default:
	}
}

// accept performs cooldown check, duplicate check and persistent
// enqueue in one step (§6, §14–16, §20).
func (q *queueService) accept(j *Job, now time.Time) error {
	if _, ok, err := q.store.GetByKey(j.RealmID, j.JobID); err != nil {
		return err
	} else if ok {
		return ErrDuplicate
	}
	if key := j.cooldownKey(); key != "" {
		if _, rem := q.cooldownRemaining(key, now); rem > 0 {
			return &cooldownError{retry: rem}
		}
	}
	if _, err := q.store.Add(j, now); err != nil {
		return err
	}
	if key := j.cooldownKey(); key != "" {
		q.recordCooldown(key, now)
	}
	q.wake()
	return nil
}

// cooldownRemaining returns the seconds until the player may send the
// next free KI request (0 = allowed).
func (q *queueService) cooldownRemaining(key string, now time.Time) (bool, time.Duration) {
	q.mu.Lock()
	defer q.mu.Unlock()
	last, ok := q.cooldowns[key]
	if !ok {
		return true, 0
	}
	next := last.Add(q.cfg.PlayerCooldown)
	if next.After(now) {
		return false, next.Sub(now)
	}
	delete(q.cooldowns, key)
	return true, 0
}

func (q *queueService) recordCooldown(key string, now time.Time) {
	q.mu.Lock()
	defer q.mu.Unlock()
	q.cooldowns[key] = now
}

// nextPending selects the highest-priority pending job (weight desc,
// arrival order asc within the same weight — §5), marks it PROCESSING
// and returns it.
func (q *queueService) nextPending() *Job {
	jobs, broken, err := q.store.LoadPending()
	if err != nil {
		log.Printf("coordinator: load queue: %v", err)
		return nil
	}
	for _, b := range broken {
		log.Printf("coordinator: broken/missing job file %q quarantined", b)
		if err := q.store.Quarantine(b); err != nil {
			log.Printf("coordinator: quarantine %q failed: %v", b, err)
		}
	}
	var cands []*Job
	for _, j := range jobs {
		if j.Status != jobPending {
			continue
		}
		cands = append(cands, j)
	}
	if len(cands) == 0 {
		return nil
	}
	sort.Slice(cands, func(i, k int) bool {
		if cands[i].Priority != cands[k].Priority {
			return cands[i].Priority > cands[k].Priority
		}
		return cands[i].File < cands[k].File
	})
	j := cands[0]
	j.Status = jobProcessing
	j.UpdatedAt = time.Now()
	if err := q.store.Save(j); err != nil {
		log.Printf("coordinator: mark processing %s: %v", j.File, err)
		return nil
	}
	return j
}

// buildMessages and outputValidation in queue.go delegate to the
// validator in validate.go (kept separate for testability).
func (q *queueService) buildMessages(j *Job) ([]chatMessage, validation) {
	v := newValidator(q.cfg)
	return v.buildMessages(j)
}

func (q *queueService) outputValidation(j *Job, ans string) validation {
	v := newValidator(q.cfg)
	return v.outputValidation(j, ans)
}

// process executes one job with the bounded correction/retry loop
// (§10–12). Each Ollama call gets its own timeout; failures consume the
// same MaxAttempts budget and never create unrelated AI work (§13).
func (q *queueService) process(j *Job) {
	q.mu.Lock()
	q.active++
	q.mu.Unlock()
	defer func() {
		q.mu.Lock()
		q.active--
		q.mu.Unlock()
	}()

	messages, v := q.buildMessages(j)
	if !v.OK {
		q.fail(j, v.Reason)
		return
	}
	for attempt := 0; attempt < j.MaxAttempts; attempt++ {
		j.Attempts = attempt + 1
		j.UpdatedAt = time.Now()
		_ = q.store.Save(j)

		ctx := context.Background()
		ans, cls := q.ollama.chat(ctx, messages)
		if cls != "" {
			// Transport failure (TIMEOUT / AI_UNAVAILABLE): retry the
			// same request within the remaining budget (§12).
			if attempt+1 >= j.MaxAttempts {
				q.fail(j, cls)
				return
			}
			continue
		}
		ov := q.outputValidation(j, ans)
		if ov.OK {
			q.complete(j, ans)
			return
		}
		if attempt+1 >= j.MaxAttempts {
			q.fail(j, CodeValidationFailed)
			return
		}
		// Bounded correction (§10/11): ask for a rule-conform
		// alternative, never silently accept rule-breaking output.
		messages = append(messages,
			chatMessage{Role: "assistant", Content: ans},
			chatMessage{Role: "user", Content: correctionPrompt(ov.Reason)},
		)
	}
	q.fail(j, CodeValidationFailed)
}

// complete persists the terminal COMPLETED state, moves the job to the
// done area and reports it to the realm.
func (q *queueService) complete(j *Job, result string) {
	j.Status = jobCompleted
	j.Result = result
	j.ErrorCode = ""
	j.UpdatedAt = time.Now()
	q.finish(j)
}

// fail persists the terminal FAILED state with a fixed fallback code
// (section 11–13) and reports it to the realm.
func (q *queueService) fail(j *Job, code string) {
	j.Status = jobFailed
	j.ErrorCode = code
	j.UpdatedAt = time.Now()
	q.finish(j)
}

func (q *queueService) finish(j *Job) {
	if err := q.store.saveDone(j); err != nil {
		log.Printf("coordinator: finalize %s: %v", j.File, err)
	}
	q.mu.Lock()
	if j.Status == jobCompleted {
		q.completed++
	} else {
		q.failed++
	}
	q.mu.Unlock()
	q.reportRealm(j)
}

// reportRealm pushes the terminal result to the responsible realm
// (result URL) unless the realm polls instead. Delivery failures are
// logged; the job stays queryable (sections 18/24/26).
func (q *queueService) reportRealm(j *Job) {
	cred, ok := q.cfg.Realms[j.RealmID]
	if !ok {
		return
	}
	res := realmResult{
		RealmID: j.RealmID,
		JobID:   j.JobID,
		JobType: j.JobType,
		Status:  j.Status,
		Result:  j.Result,
	}
	if j.Status == jobFailed {
		res.ErrorCode = j.ErrorCode
		res.ErrorMsg = codeText(j.ErrorCode)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := q.realms.deliver(ctx, cred, res); err != nil {
		log.Printf("coordinator: realm result delivery for %s/%s: %v", j.RealmID, j.JobID, err)
	}
}

// metrics snapshots the coordinator state for /status.
func (q *queueService) metrics() map[string]any {
	pending, done, quar, err := q.store.Stats()
	if err != nil {
		log.Printf("coordinator: stats: %v", err)
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	return map[string]any{
		"status":   "ok",
		"uptime_s": int(time.Since(q.start).Seconds()),
		"workers":  q.cfg.Workers,
		"queue": map[string]int{
			"pending":    pending,
			"done":       done,
			"quarantine": quar,
		},
		"active_requests": q.active,
		"completed":       q.completed,
		"failed":          q.failed,
		"ollama": map[string]any{
			"up":         q.lastOK,
			"checked_at": q.lastOKAt,
			"model":      q.cfg.OllamaModel,
			"url":        q.cfg.OllamaURL,
		},
	}
}

// probeOllama refreshes the availability flag used by /status.
func (q *queueService) probeOllama() {
	ok := q.ollama.ping(context.Background())
	q.mu.Lock()
	q.lastOK = ok
	q.lastOKAt = time.Now()
	q.mu.Unlock()
}
