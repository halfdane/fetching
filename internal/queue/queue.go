// Package queue provides a persistent SQLite-backed job queue for
// Spotify download requests.
//
// Under the hood it uses goqite (SQS-inspired, SQLite-backed) for message
// delivery, retry, and crash recovery.  A separate `jobs` table provides
// the status view required by the web UI.
package queue

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"maragu.dev/goqite"

	_ "github.com/mattn/go-sqlite3"
)

// Status represents the current state of a job.
type Status string

const (
	StatusPending Status = "pending"
	StatusRunning Status = "running"
	StatusDone    Status = "done"
	StatusFailed  Status = "failed"
)

// Job is a single download request with status information.
type Job struct {
	ID              int64      `json:"id"`
	SpotifyURI      string     `json:"spotify_uri"`
	Status          Status     `json:"status"`
	Error           string     `json:"error,omitempty"`
	RetryCount      int        `json:"retry_count"`
	FallbackQuality bool       `json:"fallback_quality"`
	CreatedAt       time.Time  `json:"created_at"`
	StartedAt       *time.Time `json:"started_at,omitempty"`
	CompletedAt     *time.Time `json:"completed_at,omitempty"`
	// Result holds the serialised CollectionView snapshot saved when the job
	// completed successfully. Used to restore UI history after a restart.
	Result json.RawMessage `json:"result,omitempty"`
}

// maxJobs is the maximum number of job rows kept in the database.
// When a new job would exceed this limit, the oldest terminal (done/failed)
// jobs are auto-trimmed. If there are no terminal jobs to trim, ErrQueueFull
// is returned and the caller should surface an error to the user.
const maxJobs = 100

// ErrQueueFull is returned by Enqueue when the job queue has reached its
// maximum capacity and no terminal jobs are available to trim.
var ErrQueueFull = errors.New("job queue is full — all 100 slots are occupied by pending or running jobs")

// retryDelays defines the wait time before each successive retry attempt.
var retryDelays = []time.Duration{
	1 * time.Second,
	15 * time.Second,
	44 * time.Second,
}

// maxRetries returns how many explicit retries are allowed before a job is permanently failed.
// Evaluated at call-time so tests can override retryDelays cleanly.
func maxRetries() int { return len(retryDelays) }

// jobTimeout is the goqite visibility timeout: if a running job isn't acked
// within this window (e.g. worker crash), the message becomes available again.
const jobTimeout = 15 * time.Minute

// goqitePayload is what we store in the goqite message body.
type goqitePayload struct {
	JobID int64 `json:"job_id"`
}

// Queue manages jobs via goqite + an auxiliary status table.
type Queue struct {
	db     *sql.DB
	gq     *goqite.Queue
	notify chan struct{}
}

// New opens (or creates) a SQLite database at the given path and initialises
// both the goqite table and the jobs status table.
func New(dbPath string) (*Queue, error) {
	db, err := sql.Open("sqlite3", dbPath+"?_journal_mode=WAL&_busy_timeout=5000")
	if err != nil {
		return nil, fmt.Errorf("open queue database: %w", err)
	}
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)

	if err := migrate(db); err != nil {
		db.Close()
		return nil, err
	}

	gq := goqite.New(goqite.NewOpts{
		DB:         db,
		Name:       "spotify",
		MaxReceive: 5,
		Timeout:    jobTimeout,
	})

	return &Queue{db: db, gq: gq, notify: make(chan struct{}, 1)}, nil
}

func migrate(db *sql.DB) error {
	// goqite schema (SQLite flavour — must match library version).
	const goqiteSchema = `
	CREATE TABLE IF NOT EXISTS goqite (
		id       TEXT PRIMARY KEY DEFAULT ('m_' || lower(hex(randomblob(16)))),
		created  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ')),
		updated  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ')),
		queue    TEXT NOT NULL,
		body     BLOB NOT NULL,
		timeout  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ')),
		received INTEGER NOT NULL DEFAULT 0,
		priority INTEGER NOT NULL DEFAULT 0
	) STRICT;
	CREATE TRIGGER IF NOT EXISTS goqite_updated_timestamp
		AFTER UPDATE ON goqite BEGIN
		UPDATE goqite SET updated = strftime('%Y-%m-%dT%H:%M:%fZ') WHERE id = old.id;
	END;
	CREATE INDEX IF NOT EXISTS goqite_queue_priority_created_idx
		ON goqite (queue, priority DESC, created);
	`

	// Status/observability table for the web UI.
	const jobsSchema = `
	CREATE TABLE IF NOT EXISTS jobs (
		id           INTEGER PRIMARY KEY AUTOINCREMENT,
		spotify_uri  TEXT    NOT NULL,
		status       TEXT    NOT NULL DEFAULT 'pending',
		error        TEXT    NOT NULL DEFAULT '',
		retry_count  INTEGER NOT NULL DEFAULT 0,
		created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
		started_at   TEXT,
		completed_at TEXT
	);
	CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
	`

	for _, ddl := range []string{goqiteSchema, jobsSchema} {
		if _, err := db.Exec(ddl); err != nil {
			return fmt.Errorf("migrate queue database: %w", err)
		}
	}

	// Additive column migrations — safe to re-run; SQLite returns an error when
	// a column already exists, which we treat as a no-op.
	columnMigrations := []string{
		`ALTER TABLE jobs ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0`,
		`ALTER TABLE jobs ADD COLUMN fallback_quality INTEGER NOT NULL DEFAULT 0`,
		// Stores the goqite message ID while the job is running, eliminating
		// the need to scan the entire goqite table to find the message on completion.
		`ALTER TABLE jobs ADD COLUMN goqite_msg_id TEXT NOT NULL DEFAULT ''`,
		// Stores the final CollectionView JSON snapshot so UI history survives restarts.
		`ALTER TABLE jobs ADD COLUMN result TEXT NOT NULL DEFAULT ''`,
	}
	for _, stmt := range columnMigrations {
		if _, err := db.Exec(stmt); err != nil {
			// "duplicate column name" means the column is already present — fine.
			if !isDuplicateColumnErr(err) {
				return fmt.Errorf("column migration %q: %w", stmt, err)
			}
		}
	}

	return nil
}

// isDuplicateColumnErr reports whether err is the SQLite "duplicate column
// name" error returned when ALTER TABLE ADD COLUMN is run a second time.
func isDuplicateColumnErr(err error) bool {
	return err != nil && strings.Contains(err.Error(), "duplicate column name")
}

// RecoverStuckJobs resets all jobs that were left in 'running' state (e.g. due
// to a server crash) back to 'pending' and re-enqueues them for immediate
// processing. It is safe to call on an empty database or when no jobs are stuck.
// It must be called before the worker begins polling.
func (q *Queue) RecoverStuckJobs() error {
	ctx := context.Background()

	rows, err := q.db.QueryContext(ctx,
		`SELECT id FROM jobs WHERE status = 'running'`)
	if err != nil {
		return fmt.Errorf("list stuck jobs: %w", err)
	}
	defer rows.Close()

	var ids []int64
	for rows.Next() {
		var id int64
		if err := rows.Scan(&id); err != nil {
			return fmt.Errorf("scan stuck job id: %w", err)
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterate stuck jobs: %w", err)
	}

	for _, id := range ids {
		_, err := q.db.ExecContext(ctx,
			// Clear goqite_msg_id: the old invisible message is no longer ours to delete.
			`UPDATE jobs SET status = 'pending', started_at = NULL, goqite_msg_id = '' WHERE id = ?`, id)
		if err != nil {
			return fmt.Errorf("reset stuck job %d: %w", id, err)
		}

		body, _ := json.Marshal(goqitePayload{JobID: id})
		if err := q.gq.Send(ctx, goqite.Message{Body: body}); err != nil {
			return fmt.Errorf("re-enqueue stuck job %d: %w", id, err)
		}

		slog.Info("queue: recovered stuck job", "id", id)
	}
	return nil
}

// EnqueueOptions configures how a batch of URIs is enqueued.
type EnqueueOptions struct {
	// FallbackQuality enables successive fallback to lower-quality audio
	// candidates when all retries for the best available file are exhausted.
	FallbackQuality bool
}

// Enqueue adds one or more Spotify URIs to the queue.
// If the total job count would exceed maxJobs, the oldest terminal (done/failed)
// jobs are automatically deleted to make room. The IDs of any auto-trimmed jobs
// are returned as trimmedIDs so callers can evict them from in-memory stores.
// ErrQueueFull is returned when there are not enough terminal jobs to trim.
func (q *Queue) Enqueue(opts EnqueueOptions, uris ...string) (jobs []*Job, trimmedIDs []int64, err error) {
	ctx := context.Background()

	// Auto-trim oldest terminal jobs when the new URIs would push us over the limit.
	var total int
	if err := q.db.QueryRowContext(ctx, `SELECT COUNT(*) FROM jobs`).Scan(&total); err != nil {
		return nil, nil, fmt.Errorf("count jobs: %w", err)
	}
	if need := (total + len(uris)) - maxJobs; need > 0 {
		rows, err := q.db.QueryContext(ctx,
			`SELECT id FROM jobs WHERE status IN ('done', 'failed') ORDER BY created_at ASC, id ASC LIMIT ?`, need)
		if err != nil {
			return nil, nil, fmt.Errorf("find terminal jobs to trim: %w", err)
		}
		for rows.Next() {
			var id int64
			if err := rows.Scan(&id); err != nil {
				rows.Close()
				return nil, nil, err
			}
			trimmedIDs = append(trimmedIDs, id)
		}
		rows.Close()
		if len(trimmedIDs) < need {
			return nil, nil, ErrQueueFull
		}
		for _, id := range trimmedIDs {
			if _, err := q.db.ExecContext(ctx, `DELETE FROM jobs WHERE id = ?`, id); err != nil {
				return nil, nil, fmt.Errorf("auto-trim job %d: %w", id, err)
			}
		}
	}

	for _, uri := range uris {
		fq := 0
		if opts.FallbackQuality {
			fq = 1
		}
		res, err := q.db.ExecContext(ctx,
			`INSERT INTO jobs (spotify_uri, fallback_quality) VALUES (?, ?)`, uri, fq)
		if err != nil {
			return nil, trimmedIDs, fmt.Errorf("insert job for %q: %w", uri, err)
		}
		jobID, _ := res.LastInsertId()

		body, _ := json.Marshal(goqitePayload{JobID: jobID})
		if err := q.gq.Send(ctx, goqite.Message{Body: body}); err != nil {
			return nil, trimmedIDs, fmt.Errorf("send goqite message for job %d: %w", jobID, err)
		}

		jobs = append(jobs, &Job{
			ID:              jobID,
			SpotifyURI:      uri,
			Status:          StatusPending,
			FallbackQuality: opts.FallbackQuality,
			CreatedAt:       time.Now(),
		})
	}

	// Wake the worker if it's waiting for work.
	select {
	case q.notify <- struct{}{}:
	default:
	}

	return jobs, trimmedIDs, nil
}

// ClearDone deletes all completed (done) jobs from the database and returns
// their IDs so callers can evict them from in-memory stores.
// Failed jobs are intentionally kept so they remain visible for investigation.
func (q *Queue) ClearDone() ([]int64, error) {
	ctx := context.Background()
	rows, err := q.db.QueryContext(ctx, `SELECT id FROM jobs WHERE status IN ('done', 'failed')`)
	if err != nil {
		return nil, fmt.Errorf("list done jobs: %w", err)
	}
	var ids []int64
	for rows.Next() {
		var id int64
		if err := rows.Scan(&id); err != nil {
			rows.Close()
			return nil, err
		}
		ids = append(ids, id)
	}
	rows.Close()
	       for _, id := range ids {
		       if _, err := q.db.ExecContext(ctx, `DELETE FROM jobs WHERE id = ?`, id); err != nil {
			       return nil, fmt.Errorf("delete terminal job %d: %w", id, err)
		       }
	       }
	       return ids, nil
}

// Next claims and returns the next pending job, or nil if the queue is empty.
func (q *Queue) Next() (*Job, error) {
	ctx := context.Background()

	msg, err := q.gq.Receive(ctx)
	if err != nil {
		return nil, fmt.Errorf("receive from queue: %w", err)
	}
	if msg == nil {
		return nil, nil
	}

	var payload goqitePayload
	if err := json.Unmarshal(msg.Body, &payload); err != nil {
		_ = q.gq.Delete(ctx, msg.ID)
		return nil, fmt.Errorf("parse goqite message payload: %w", err)
	}

	// Guard: discard stale messages for jobs that are already done or failed.
	// This can happen when RecoverStuckJobs sent a fresh message and the original
	// invisible message later resurfaced after the visibility timeout.
	var currentStatus Status
	if err := q.db.QueryRowContext(ctx,
		`SELECT status FROM jobs WHERE id = ?`, payload.JobID,
	).Scan(&currentStatus); err != nil {
		_ = q.gq.Delete(ctx, msg.ID)
		return nil, fmt.Errorf("check job %d status: %w", payload.JobID, err)
	}
	if currentStatus == StatusDone || currentStatus == StatusFailed {
		_ = q.gq.Delete(ctx, msg.ID)
		return nil, nil
	}

	now := time.Now().UTC().Format(time.RFC3339)
	_, err = q.db.ExecContext(ctx,
		`UPDATE jobs SET status = 'running', started_at = ?, goqite_msg_id = ? WHERE id = ?`,
		now, string(msg.ID), payload.JobID,
	)
	if err != nil {
		return nil, fmt.Errorf("mark job %d running: %w", payload.JobID, err)
	}

	row := q.db.QueryRowContext(ctx,
		`SELECT id, spotify_uri, status, error, retry_count, fallback_quality, created_at, started_at, completed_at, result
		 FROM jobs WHERE id = ?`, payload.JobID)

	job, err := scanJob(row)
	if err != nil {
		return nil, fmt.Errorf("scan job %d: %w", payload.JobID, err)
	}
	return job, nil
}

// Complete marks a job as done and removes its goqite message.
func (q *Queue) Complete(id int64) error {
	return q.finishJob(id, StatusDone, "")
}

// Fail marks a job as failed. If retries remain the job is re-enqueued with
// exponential back-off; otherwise it is permanently marked failed.
func (q *Queue) Fail(id int64, errMsg string) error {
	return q.finishJob(id, StatusFailed, errMsg)
}

// StoreResult persists a serialised CollectionView snapshot against a
// completed job so the UI can restore full history after a restart.
func (q *Queue) StoreResult(id int64, data json.RawMessage) error {
	_, err := q.db.ExecContext(context.Background(),
		`UPDATE jobs SET result = ? WHERE id = ?`, string(data), id)
	return err
}

func (q *Queue) finishJob(id int64, finalStatus Status, errMsg string) error {
	ctx := context.Background()

	// Load current retry count and stored goqite message ID in one query.
	var retryCount int
	var msgID string
	if err := q.db.QueryRowContext(ctx,
		`SELECT retry_count, goqite_msg_id FROM jobs WHERE id = ?`, id,
	).Scan(&retryCount, &msgID); err != nil {
		return fmt.Errorf("load job %d: %w", id, err)
	}

	if finalStatus == StatusFailed && retryCount < maxRetries() {
		delay := retryDelays[retryCount]
		body, _ := json.Marshal(goqitePayload{JobID: id})

		if msgID != "" {
			_ = q.gq.Delete(ctx, goqite.ID(msgID))
		}
		if err := q.gq.Send(ctx, goqite.Message{Body: body, Delay: delay}); err != nil {
			return fmt.Errorf("re-enqueue job %d: %w", id, err)
		}

		_, err := q.db.ExecContext(ctx,
			`UPDATE jobs SET status = 'pending', retry_count = retry_count + 1, error = ? WHERE id = ?`,
			errMsg, id)
		return err
	}

	if msgID != "" {
		_ = q.gq.Delete(ctx, goqite.ID(msgID))
	}

	now := time.Now().UTC().Format(time.RFC3339)
	_, err := q.db.ExecContext(ctx,
		`UPDATE jobs SET status = ?, error = ?, completed_at = ?, goqite_msg_id = '' WHERE id = ?`,
		finalStatus, errMsg, now, id)
	return err
}

// RetryResult holds the outcome of a Retry call.
type RetryResult struct {
	NewJob     *Job
	DeletedIDs []int64 // previous terminal job IDs removed from the DB
}

// Retry enqueues a fresh job for the given URI after deleting any existing
// terminal-state (done/failed) jobs for it. This is the "clean clone" pattern:
// old rows are removed so the UI shows only the new pending job.
func (q *Queue) Retry(opts EnqueueOptions, uri string) (*RetryResult, error) {
	ctx := context.Background()

	rows, err := q.db.QueryContext(ctx,
		`SELECT id FROM jobs WHERE spotify_uri = ? AND status IN ('done', 'failed')`, uri)
	if err != nil {
		return nil, fmt.Errorf("find terminal jobs for %q: %w", uri, err)
	}
	var deletedIDs []int64
	for rows.Next() {
		var id int64
		if err := rows.Scan(&id); err != nil {
			rows.Close()
			return nil, err
		}
		deletedIDs = append(deletedIDs, id)
	}
	rows.Close()

	for _, id := range deletedIDs {
		if _, err := q.db.ExecContext(ctx, `DELETE FROM jobs WHERE id = ?`, id); err != nil {
			return nil, fmt.Errorf("delete old job %d: %w", id, err)
		}
	}

	newJobs, autoTrimmed, err := q.Enqueue(opts, uri)
	if err != nil {
		return nil, err
	}
	return &RetryResult{NewJob: newJobs[0], DeletedIDs: append(deletedIDs, autoTrimmed...)}, nil
}

// List returns all jobs ordered by creation time descending.
func (q *Queue) List() ([]*Job, error) {
	rows, err := q.db.QueryContext(context.Background(),
		`SELECT id, spotify_uri, status, error, retry_count, fallback_quality, created_at, started_at, completed_at, result
		 FROM jobs ORDER BY created_at DESC, id DESC`)
	if err != nil {
		return nil, fmt.Errorf("list jobs: %w", err)
	}
	defer rows.Close()

	var jobs []*Job
	for rows.Next() {
		job, err := scanJob(rows)
		if err != nil {
			return nil, err
		}
		jobs = append(jobs, job)
	}
	return jobs, rows.Err()
}

// Close closes the underlying database.
func (q *Queue) Close() error {
	return q.db.Close()
}

// Notify returns a channel that is signalled (non-blocking) whenever new work
// is enqueued. Workers can select on this to wake up immediately instead of
// polling on a timer.
func (q *Queue) Notify() <-chan struct{} {
	return q.notify
}

// scanner is satisfied by both *sql.Row and *sql.Rows.
type scanner interface {
	Scan(dest ...any) error
}

func scanJob(s scanner) (*Job, error) {
	var j Job
	var startedAt, completedAt, result sql.NullString
	var createdAt string
	var fallbackQualityInt int
	if err := s.Scan(
		&j.ID, &j.SpotifyURI, &j.Status, &j.Error, &j.RetryCount, &fallbackQualityInt,
		&createdAt, &startedAt, &completedAt, &result,
	); err != nil {
		return nil, err
	}
	j.FallbackQuality = fallbackQualityInt != 0
	if result.Valid && result.String != "" {
		j.Result = json.RawMessage(result.String)
	}

	if t, err := time.Parse(time.RFC3339, createdAt); err == nil {
		j.CreatedAt = t
	} else if t, err := time.Parse("2006-01-02 15:04:05", createdAt); err == nil {
		j.CreatedAt = t
	}
	if startedAt.Valid {
		if t, err := time.Parse(time.RFC3339, startedAt.String); err == nil {
			j.StartedAt = &t
		}
	}
	if completedAt.Valid {
		if t, err := time.Parse(time.RFC3339, completedAt.String); err == nil {
			j.CompletedAt = &t
		}
	}
	return &j, nil
}
