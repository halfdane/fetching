package queue

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	"maragu.dev/goqite"
)

// newTestQueue creates an in-memory queue suitable for tests.
func newTestQueue(t *testing.T) *Queue {
	t.Helper()
	q, err := New(":memory:")
	if err != nil {
		t.Fatalf("create test queue: %v", err)
	}
	t.Cleanup(func() { q.Close() })
	return q
}

// TestRecoverStuckJobs_NoOp verifies that RecoverStuckJobs is safe on an empty DB.
func TestRecoverStuckJobs_NoOp(t *testing.T) {
	q := newTestQueue(t)
	if err := q.RecoverStuckJobs(); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

// TestRecoverStuckJobs_ResetsRunningToPending verifies that a 'running' job is
// reset to 'pending' and becomes available for immediate processing.
func TestRecoverStuckJobs_ResetsRunningToPending(t *testing.T) {
	q := newTestQueue(t)

	// Enqueue and claim a job (sets status → running).
	jobs, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	if err != nil {
		t.Fatalf("enqueue: %v", err)
	}
	jobID := jobs[0].ID

	claimed, err := q.Next()
	if err != nil || claimed == nil {
		t.Fatalf("Next(): job=%v err=%v", claimed, err)
	}
	if claimed.Status != StatusRunning {
		t.Fatalf("expected running, got %s", claimed.Status)
	}

	// Simulate crash: don't call Complete/Fail; just call RecoverStuckJobs.
	if err := q.RecoverStuckJobs(); err != nil {
		t.Fatalf("RecoverStuckJobs: %v", err)
	}

	// The job should now be 'pending' in the DB.
	var status Status
	if err := q.db.QueryRow(`SELECT status FROM jobs WHERE id = ?`, jobID).Scan(&status); err != nil {
		t.Fatalf("query status: %v", err)
	}
	if status != StatusPending {
		t.Errorf("expected pending after recovery, got %s", status)
	}
}

// TestRecoverStuckJobs_ReenqueuesImmediately verifies that a fresh goqite message
// is available immediately after RecoverStuckJobs (not after the visibility timeout).
func TestRecoverStuckJobs_ReenqueuesImmediately(t *testing.T) {
	q := newTestQueue(t)

	if _, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:abc"); err != nil {
		t.Fatalf("enqueue: %v", err)
	}

	// Claim the job (makes original message invisible).
	if _, err := q.Next(); err != nil {
		t.Fatalf("Next(): %v", err)
	}

	// Recover: should send a new message, immediately available.
	if err := q.RecoverStuckJobs(); err != nil {
		t.Fatalf("RecoverStuckJobs: %v", err)
	}

	// The recovery-sent message should be immediately receivable.
	recovered, err := q.Next()
	if err != nil {
		t.Fatalf("Next() after recovery: %v", err)
	}
	if recovered == nil {
		t.Fatal("expected recovered job, got nil")
	}
	if recovered.Status != StatusRunning {
		t.Errorf("expected running, got %s", recovered.Status)
	}
}

// TestRecoverStuckJobs_DoesNotTouchDoneOrFailed verifies that completed or
// permanently-failed jobs are not affected.
func TestRecoverStuckJobs_DoesNotTouchDoneOrFailed(t *testing.T) {
	q := newTestQueue(t)

	jobs, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:done", "spotify:track:failed")
	if err != nil {
		t.Fatalf("enqueue: %v", err)
	}
	doneID := jobs[0].ID
	failedID := jobs[1].ID

	// Directly set terminal statuses (avoids retry delay timing issues).
	if _, err := q.db.Exec(`UPDATE jobs SET status = 'done', completed_at = datetime('now') WHERE id = ?`, doneID); err != nil {
		t.Fatalf("set done: %v", err)
	}
	if _, err := q.db.Exec(`UPDATE jobs SET status = 'failed', completed_at = datetime('now') WHERE id = ?`, failedID); err != nil {
		t.Fatalf("set failed: %v", err)
	}

	if err := q.RecoverStuckJobs(); err != nil {
		t.Fatalf("RecoverStuckJobs: %v", err)
	}

	var doneStatus, failedStatus Status
	q.db.QueryRow(`SELECT status FROM jobs WHERE id = ?`, doneID).Scan(&doneStatus)
	q.db.QueryRow(`SELECT status FROM jobs WHERE id = ?`, failedID).Scan(&failedStatus)

	if doneStatus != StatusDone {
		t.Errorf("done job changed to %s", doneStatus)
	}
	if failedStatus != StatusFailed {
		t.Errorf("failed job changed to %s", failedStatus)
	}
}

// TestRecoverStuckJobs_MultipleJobs verifies all stuck jobs are recovered.
func TestRecoverStuckJobs_MultipleJobs(t *testing.T) {
	q := newTestQueue(t)

	if _, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:1", "spotify:track:2", "spotify:track:3"); err != nil {
		t.Fatalf("enqueue: %v", err)
	}

	// Claim all three (sets them all to running).
	for i := 0; i < 3; i++ {
		if _, err := q.Next(); err != nil {
			t.Fatalf("Next() %d: %v", i, err)
		}
	}

	if err := q.RecoverStuckJobs(); err != nil {
		t.Fatalf("RecoverStuckJobs: %v", err)
	}

	var pendingCount int
	if err := q.db.QueryRow(`SELECT count(*) FROM jobs WHERE status = 'pending'`).Scan(&pendingCount); err != nil {
		t.Fatalf("count: %v", err)
	}
	if pendingCount != 3 {
		t.Errorf("expected 3 pending jobs, got %d", pendingCount)
	}
}

// TestNext_DiscardsStaleMessageForDoneJob verifies that Next() silently discards
// a goqite message whose job is already 'done' (the stale-message scenario).
func TestNext_DiscardsStaleMessageForDoneJob(t *testing.T) {
	q := newTestQueue(t)

	jobs, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	if err != nil {
		t.Fatalf("enqueue: %v", err)
	}
	jobID := jobs[0].ID

	// Pick up and complete the job (deletes original goqite message).
	j, err := q.Next()
	if err != nil || j == nil {
		t.Fatalf("Next(): %v %v", j, err)
	}
	if err := q.Complete(j.ID); err != nil {
		t.Fatalf("Complete: %v", err)
	}

	// Inject a stale goqite message directly (simulates the original invisible
	// message resurfacing after the 15-min visibility timeout).
	body, _ := json.Marshal(goqitePayload{JobID: jobID})
	if err := q.gq.Send(context.Background(), goqite.Message{Body: body}); err != nil {
		t.Fatalf("inject stale message: %v", err)
	}

	// Next() must discard the stale message and return nil.
	got, err := q.Next()
	if err != nil {
		t.Fatalf("Next() on stale message: %v", err)
	}
	if got != nil {
		t.Errorf("expected nil (stale discarded), got job id=%d status=%s", got.ID, got.Status)
	}
}

// TestNext_DiscardsStaleMessageForFailedJob verifies the guard discards messages
// for permanently-failed jobs.
func TestNext_DiscardsStaleMessageForFailedJob(t *testing.T) {
	q := newTestQueue(t)

	jobs, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	if err != nil {
		t.Fatalf("enqueue: %v", err)
	}
	jobID := jobs[0].ID

	// Directly mark the job as failed without consuming the goqite message.
	// This simulates the stale-message scenario: the goqite message is still
	// present but the job has already been completed and marked failed.
	if _, err := q.db.Exec(`UPDATE jobs SET status = 'failed', completed_at = datetime('now') WHERE id = ?`, jobID); err != nil {
		t.Fatalf("set failed: %v", err)
	}

	// Next() should see 'failed' status for the pending message and discard it.
	got, err := q.Next()
	if err != nil {
		t.Fatalf("Next() on failed-job message: %v", err)
	}
	if got != nil {
		t.Errorf("expected nil (stale discarded), got job id=%d status=%s", got.ID, got.Status)
	}
}

// ---- Enqueue ----

// TestEnqueue_CreatesPendingJob verifies Enqueue inserts a job with pending status.
func TestEnqueue_CreatesPendingJob(t *testing.T) {
	q := newTestQueue(t)
	jobs, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	if err != nil {
		t.Fatalf("Enqueue: %v", err)
	}
	if len(jobs) != 1 {
		t.Fatalf("expected 1 job, got %d", len(jobs))
	}
	if jobs[0].Status != StatusPending {
		t.Errorf("status = %s, want pending", jobs[0].Status)
	}
	if jobs[0].SpotifyURI != "spotify:track:abc" {
		t.Errorf("uri = %s, want spotify:track:abc", jobs[0].SpotifyURI)
	}
}

// TestEnqueue_FallbackQualityStored verifies the FallbackQuality option is persisted.
func TestEnqueue_FallbackQualityStored(t *testing.T) {
	q := newTestQueue(t)
	jobs, _, err := q.Enqueue(EnqueueOptions{FallbackQuality: true}, "spotify:track:abc")
	if err != nil {
		t.Fatalf("Enqueue: %v", err)
	}
	if !jobs[0].FallbackQuality {
		t.Error("expected FallbackQuality=true")
	}
}

// TestEnqueue_MultipleBatch verifies Enqueue with multiple URIs creates one job each.
func TestEnqueue_MultipleBatch(t *testing.T) {
	q := newTestQueue(t)
	jobs, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:a", "spotify:track:b", "spotify:track:c")
	if err != nil {
		t.Fatalf("Enqueue: %v", err)
	}
	if len(jobs) != 3 {
		t.Fatalf("expected 3 jobs, got %d", len(jobs))
	}
}

// ---- Next ----

// TestNext_EmptyQueueReturnsNil verifies Next returns nil when the queue is empty.
func TestNext_EmptyQueueReturnsNil(t *testing.T) {
	q := newTestQueue(t)
	job, err := q.Next()
	if err != nil {
		t.Fatalf("Next(): %v", err)
	}
	if job != nil {
		t.Errorf("expected nil, got job id=%d", job.ID)
	}
}

// TestNext_MarksJobRunning verifies Next transitions a pending job to running
// and populates StartedAt and GoqiteMsgID.
func TestNext_MarksJobRunning(t *testing.T) {
	q := newTestQueue(t)
	enqueued, _, _ := q.Enqueue(EnqueueOptions{}, "spotify:track:abc")

	job, err := q.Next()
	if err != nil {
		t.Fatalf("Next(): %v", err)
	}
	if job == nil {
		t.Fatal("expected a job, got nil")
	}
	if job.ID != enqueued[0].ID {
		t.Errorf("job id = %d, want %d", job.ID, enqueued[0].ID)
	}
	if job.Status != StatusRunning {
		t.Errorf("status = %s, want running", job.Status)
	}
	if job.StartedAt == nil {
		t.Error("started_at should be set")
	}

	// goqite_msg_id should be populated after Next().
	var msgID string
	q.db.QueryRow(`SELECT goqite_msg_id FROM jobs WHERE id = ?`, job.ID).Scan(&msgID)
	if msgID == "" {
		t.Error("goqite_msg_id should be set after Next()")
	}
}

// TestNext_SecondCallReturnsNilWhileRunning verifies Next returns nil when the
// only job is already running (visibility timeout not expired).
func TestNext_SecondCallReturnsNilWhileRunning(t *testing.T) {
	q := newTestQueue(t)
	q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	q.Next() // claim it

	second, err := q.Next()
	if err != nil {
		t.Fatalf("second Next(): %v", err)
	}
	if second != nil {
		t.Errorf("expected nil (job already running), got id=%d", second.ID)
	}
}

// ---- Complete ----

// TestComplete_MarksJobDone verifies Complete sets status to done and clears msg id.
func TestComplete_MarksJobDone(t *testing.T) {
	q := newTestQueue(t)
	q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	job, _ := q.Next()

	if err := q.Complete(job.ID); err != nil {
		t.Fatalf("Complete: %v", err)
	}

	var status string
	var msgID string
	q.db.QueryRow(`SELECT status, goqite_msg_id FROM jobs WHERE id = ?`, job.ID).
		Scan(&status, &msgID)
	if Status(status) != StatusDone {
		t.Errorf("status = %s, want done", status)
	}
	if msgID != "" {
		t.Errorf("goqite_msg_id should be cleared after Complete, got %q", msgID)
	}
}

// ---- Fail ----

// TestFail_WithRetriesRemaining verifies Fail re-enqueues with incremented retry_count.
func TestFail_WithRetriesRemaining(t *testing.T) {
	origDelays := retryDelays
	retryDelays = []time.Duration{1 * time.Millisecond}
	defer func() { retryDelays = origDelays }()

	q := newTestQueue(t)
	q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	job, _ := q.Next()

	if err := q.Fail(job.ID, "transient error"); err != nil {
		t.Fatalf("Fail: %v", err)
	}

	var status Status
	var retryCount int
	q.db.QueryRow(`SELECT status, retry_count FROM jobs WHERE id = ?`, job.ID).
		Scan(&status, &retryCount)
	if status != StatusPending {
		t.Errorf("status = %s, want pending (re-enqueued for retry)", status)
	}
	if retryCount != 1 {
		t.Errorf("retry_count = %d, want 1", retryCount)
	}
}

// TestFail_AfterMaxRetries verifies Fail permanently marks the job as failed
// once all retry attempts are exhausted.
func TestFail_AfterMaxRetries(t *testing.T) {
	origDelays := retryDelays
	retryDelays = []time.Duration{} // no retries
	defer func() { retryDelays = origDelays }()

	q := newTestQueue(t)
	q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	job, _ := q.Next()

	if err := q.Fail(job.ID, "permanent error"); err != nil {
		t.Fatalf("Fail: %v", err)
	}

	var status, errMsg string
	q.db.QueryRow(`SELECT status, error FROM jobs WHERE id = ?`, job.ID).
		Scan(&status, &errMsg)
	if Status(status) != StatusFailed {
		t.Errorf("status = %s, want failed", status)
	}
	if errMsg != "permanent error" {
		t.Errorf("error = %q, want 'permanent error'", errMsg)
	}
}

// ---- List ----

// TestList_OrderedNewestFirst verifies List returns all jobs newest-first.
func TestList_OrderedNewestFirst(t *testing.T) {
	q := newTestQueue(t)
	q.Enqueue(EnqueueOptions{}, "spotify:track:first")
	q.Enqueue(EnqueueOptions{}, "spotify:track:second")
	q.Enqueue(EnqueueOptions{}, "spotify:track:third")

	jobs, err := q.List()
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(jobs) != 3 {
		t.Fatalf("expected 3 jobs, got %d", len(jobs))
	}
	if jobs[0].SpotifyURI != "spotify:track:third" {
		t.Errorf("first in list = %q, want spotify:track:third (newest first)", jobs[0].SpotifyURI)
	}
	if jobs[2].SpotifyURI != "spotify:track:first" {
		t.Errorf("last in list = %q, want spotify:track:first", jobs[2].SpotifyURI)
	}
}

// TestList_Empty verifies List returns an empty (nil) slice when no jobs exist.
func TestList_Empty(t *testing.T) {
	q := newTestQueue(t)
	jobs, err := q.List()
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(jobs) != 0 {
		t.Errorf("expected 0 jobs, got %d", len(jobs))
	}
}

// ---- Retry ----

// TestRetry_DeletesTerminalJobAndCreatesNew verifies Retry removes done/failed jobs
// for a URI and enqueues a fresh one with a new ID.
func TestRetry_DeletesTerminalJobAndCreatesNew(t *testing.T) {
	origDelays := retryDelays
	retryDelays = []time.Duration{}
	defer func() { retryDelays = origDelays }()

	q := newTestQueue(t)
	enqueued, _, _ := q.Enqueue(EnqueueOptions{}, "spotify:album:xyz")
	oldID := enqueued[0].ID

	// Process and permanently fail the job.
	job, _ := q.Next()
	q.Fail(job.ID, "failed")

	result, err := q.Retry(EnqueueOptions{}, "spotify:album:xyz")
	if err != nil {
		t.Fatalf("Retry: %v", err)
	}
	if len(result.DeletedIDs) != 1 || result.DeletedIDs[0] != oldID {
		t.Errorf("DeletedIDs = %v, want [%d]", result.DeletedIDs, oldID)
	}
	if result.NewJob.ID == oldID {
		t.Error("new job should have a different ID than the deleted job")
	}
	if result.NewJob.Status != StatusPending {
		t.Errorf("new job status = %s, want pending", result.NewJob.Status)
	}

	// Old job must be gone from the DB.
	var count int
	q.db.QueryRow(`SELECT count(*) FROM jobs WHERE id = ?`, oldID).Scan(&count)
	if count != 0 {
		t.Error("old job still present in DB after Retry")
	}
}

// TestRetry_NoTerminalJobs verifies Retry works cleanly when there are no old jobs.
func TestRetry_NoTerminalJobs(t *testing.T) {
	q := newTestQueue(t)
	result, err := q.Retry(EnqueueOptions{}, "spotify:track:new")
	if err != nil {
		t.Fatalf("Retry: %v", err)
	}
	if len(result.DeletedIDs) != 0 {
		t.Errorf("expected no deleted IDs, got %v", result.DeletedIDs)
	}
	if result.NewJob.SpotifyURI != "spotify:track:new" {
		t.Errorf("uri = %s", result.NewJob.SpotifyURI)
	}
}

// TestRetry_DoesNotDeleteRunningJob verifies Retry does not touch running jobs.
func TestRetry_DoesNotDeleteRunningJob(t *testing.T) {
	q := newTestQueue(t)
	q.Enqueue(EnqueueOptions{}, "spotify:track:abc")
	job, _ := q.Next() // status → running

	result, err := q.Retry(EnqueueOptions{}, "spotify:track:abc")
	if err != nil {
		t.Fatalf("Retry: %v", err)
	}
	if len(result.DeletedIDs) != 0 {
		t.Errorf("running job should not be deleted, got DeletedIDs=%v", result.DeletedIDs)
	}

	// Running job still present.
	var status Status
	q.db.QueryRow(`SELECT status FROM jobs WHERE id = ?`, job.ID).Scan(&status)
	if status != StatusRunning {
		t.Errorf("running job status changed to %s", status)
	}
}
