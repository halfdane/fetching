package queue

import "testing"

func TestClearDoneAlsoDeletesFailed(t *testing.T) {
	q := newTestQueue(t)
	jobs, _, err := q.Enqueue(EnqueueOptions{}, "spotify:track:done", "spotify:track:failed")
	if err != nil {
		t.Fatalf("enqueue: %v", err)
	}
	// Set terminal statuses.
	if _, err := q.db.Exec(`UPDATE jobs SET status = 'done', completed_at = datetime('now') WHERE id = ?`, jobs[0].ID); err != nil {
		t.Fatalf("set done: %v", err)
	}
	if _, err := q.db.Exec(`UPDATE jobs SET status = 'failed', completed_at = datetime('now') WHERE id = ?`, jobs[1].ID); err != nil {
		t.Fatalf("set failed: %v", err)
	}

	ids, err := q.ClearDone()
	if err != nil {
		t.Fatalf("ClearDone: %v", err)
	}
	if len(ids) != 2 {
		t.Errorf("expected 2 jobs deleted, got %d", len(ids))
	}
	for _, id := range jobs {
		var count int
		q.db.QueryRow(`SELECT count(*) FROM jobs WHERE id = ?`, id.ID).Scan(&count)
		if count != 0 {
			t.Errorf("job %d still present after ClearDone", id.ID)
		}
	}
}
