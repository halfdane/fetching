package queue

import (
	"context"
	"encoding/json"
	"testing"

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
	jobs, err := q.Enqueue("spotify:track:abc")
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

	if _, err := q.Enqueue("spotify:track:abc"); err != nil {
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

	jobs, err := q.Enqueue("spotify:track:done", "spotify:track:failed")
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

	if _, err := q.Enqueue("spotify:track:1", "spotify:track:2", "spotify:track:3"); err != nil {
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

	jobs, err := q.Enqueue("spotify:track:abc")
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

	jobs, err := q.Enqueue("spotify:track:abc")
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
