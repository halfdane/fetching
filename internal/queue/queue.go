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
	"fmt"
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
	ID          int64      `json:"id"`
	SpotifyURI  string     `json:"spotify_uri"`
	Status      Status     `json:"status"`
	Error       string     `json:"error,omitempty"`
	RetryCount  int        `json:"retry_count"`
	CreatedAt   time.Time  `json:"created_at"`
	StartedAt   *time.Time `json:"started_at,omitempty"`
	CompletedAt *time.Time `json:"completed_at,omitempty"`
}

// retryDelays defines the wait time before each successive retry attempt.
var retryDelays = []time.Duration{
	1 * time.Second,
	15 * time.Second,
	44 * time.Second,
}

// maxRetries is how many explicit retries we allow before marking a job failed.
var maxRetries = len(retryDelays)

// jobTimeout is the goqite visibility timeout: if a running job isn't acked
// within this window (e.g. worker crash), the message becomes available again.
const jobTimeout = 15 * time.Minute

// goqitePayload is what we store in the goqite message body.
type goqitePayload struct {
	JobID int64 `json:"job_id"`
}

// Queue manages jobs via goqite + an auxiliary status table.
type Queue struct {
	db *sql.DB
	gq *goqite.Queue
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

	return &Queue{db: db, gq: gq}, nil
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

// Enqueue adds one or more Spotify URIs to the queue.
func (q *Queue) Enqueue(uris ...string) ([]*Job, error) {
	ctx := context.Background()
	var jobs []*Job

	for _, uri := range uris {
		res, err := q.db.ExecContext(ctx,
			`INSERT INTO jobs (spotify_uri) VALUES (?)`, uri)
		if err != nil {
			return nil, fmt.Errorf("insert job for %q: %w", uri, err)
		}
		jobID, _ := res.LastInsertId()

		body, _ := json.Marshal(goqitePayload{JobID: jobID})
		if err := q.gq.Send(ctx, goqite.Message{Body: body}); err != nil {
			return nil, fmt.Errorf("send goqite message for job %d: %w", jobID, err)
		}

		jobs = append(jobs, &Job{
			ID:         jobID,
			SpotifyURI: uri,
			Status:     StatusPending,
			CreatedAt:  time.Now(),
		})
	}
	return jobs, nil
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

	now := time.Now().UTC().Format(time.RFC3339)
	_, err = q.db.ExecContext(ctx,
		`UPDATE jobs SET status = 'running', started_at = ? WHERE id = ?`,
		now, payload.JobID,
	)
	if err != nil {
		return nil, fmt.Errorf("mark job %d running: %w", payload.JobID, err)
	}

	row := q.db.QueryRowContext(ctx,
		`SELECT id, spotify_uri, status, error, retry_count, created_at, started_at, completed_at
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

func (q *Queue) finishJob(id int64, finalStatus Status, errMsg string) error {
	ctx := context.Background()

	// Load current retry count.
	var retryCount int
	if err := q.db.QueryRowContext(ctx,
		`SELECT retry_count FROM jobs WHERE id = ?`, id,
	).Scan(&retryCount); err != nil {
		return fmt.Errorf("load retry count for job %d: %w", id, err)
	}

	// Find the goqite message ID for this job.
	msgID, err := q.findGoqiteID(ctx, id)
	if err != nil {
		return fmt.Errorf("find goqite message for job %d: %w", id, err)
	}

	if finalStatus == StatusFailed && retryCount < maxRetries {
		delay := retryDelays[retryCount]
		body, _ := json.Marshal(goqitePayload{JobID: id})

		if msgID != "" {
			_ = q.gq.Delete(ctx, goqite.ID(msgID))
		}
		if err := q.gq.Send(ctx, goqite.Message{Body: body, Delay: delay}); err != nil {
			return fmt.Errorf("re-enqueue job %d: %w", id, err)
		}

		_, err = q.db.ExecContext(ctx,
			`UPDATE jobs SET status = 'pending', retry_count = retry_count + 1, error = ? WHERE id = ?`,
			errMsg, id)
		return err
	}

	if msgID != "" {
		_ = q.gq.Delete(ctx, goqite.ID(msgID))
	}

	now := time.Now().UTC().Format(time.RFC3339)
	_, err = q.db.ExecContext(ctx,
		`UPDATE jobs SET status = ?, error = ?, completed_at = ? WHERE id = ?`,
		finalStatus, errMsg, now, id)
	return err
}

// findGoqiteID returns the goqite message ID currently associated with job id.
func (q *Queue) findGoqiteID(ctx context.Context, jobID int64) (string, error) {
	rows, err := q.db.QueryContext(ctx,
		`SELECT id, body FROM goqite WHERE queue = 'spotify'`)
	if err != nil {
		return "", err
	}
	defer rows.Close()

	for rows.Next() {
		var msgID string
		var body []byte
		if err := rows.Scan(&msgID, &body); err != nil {
			continue
		}
		var p goqitePayload
		if err := json.Unmarshal(body, &p); err != nil {
			continue
		}
		if p.JobID == jobID {
			return msgID, nil
		}
	}
	return "", nil
}

// List returns all jobs ordered by creation time descending.
func (q *Queue) List() ([]*Job, error) {
	rows, err := q.db.QueryContext(context.Background(),
		`SELECT id, spotify_uri, status, error, retry_count, created_at, started_at, completed_at
		 FROM jobs ORDER BY created_at DESC`)
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

// scanner is satisfied by both *sql.Row and *sql.Rows.
type scanner interface {
	Scan(dest ...any) error
}

func scanJob(s scanner) (*Job, error) {
	var j Job
	var startedAt, completedAt sql.NullString
	var createdAt string
	if err := s.Scan(
		&j.ID, &j.SpotifyURI, &j.Status, &j.Error, &j.RetryCount,
		&createdAt, &startedAt, &completedAt,
	); err != nil {
		return nil, err
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
