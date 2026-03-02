// Package queue provides a persistent SQLite-backed job queue for
// Spotify download requests.
package queue

import (
	"database/sql"
	"fmt"
	"time"

	_ "github.com/mattn/go-sqlite3"
)

// Status represents the current state of a job.
type Status string

const (
	StatusPending    Status = "pending"
	StatusRunning    Status = "running"
	StatusDone       Status = "done"
	StatusFailed     Status = "failed"
)

// Job is a single download request in the queue.
type Job struct {
	ID          int64     `json:"id"`
	SpotifyURI  string    `json:"spotify_uri"`
	Status      Status    `json:"status"`
	Error       string    `json:"error,omitempty"`
	CreatedAt   time.Time `json:"created_at"`
	StartedAt   *time.Time `json:"started_at,omitempty"`
	CompletedAt *time.Time `json:"completed_at,omitempty"`
}

// Queue manages jobs in a SQLite database.
type Queue struct {
	db *sql.DB
}

// New opens (or creates) a SQLite database at the given path and
// initializes the jobs table.
func New(dbPath string) (*Queue, error) {
	db, err := sql.Open("sqlite3", dbPath+"?_journal_mode=WAL&_busy_timeout=5000")
	if err != nil {
		return nil, fmt.Errorf("open queue database: %w", err)
	}

	if err := migrate(db); err != nil {
		db.Close()
		return nil, err
	}

	return &Queue{db: db}, nil
}

func migrate(db *sql.DB) error {
	const ddl = `
	CREATE TABLE IF NOT EXISTS jobs (
		id           INTEGER PRIMARY KEY AUTOINCREMENT,
		spotify_uri  TEXT    NOT NULL,
		status       TEXT    NOT NULL DEFAULT 'pending',
		error        TEXT    NOT NULL DEFAULT '',
		created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
		started_at   TEXT,
		completed_at TEXT
	);
	CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
	`
	if _, err := db.Exec(ddl); err != nil {
		return fmt.Errorf("migrate queue database: %w", err)
	}
	return nil
}

// Enqueue adds one or more Spotify URIs to the queue and returns the created jobs.
func (q *Queue) Enqueue(uris ...string) ([]*Job, error) {
	tx, err := q.db.Begin()
	if err != nil {
		return nil, fmt.Errorf("begin transaction: %w", err)
	}
	defer tx.Rollback()

	stmt, err := tx.Prepare("INSERT INTO jobs (spotify_uri) VALUES (?)")
	if err != nil {
		return nil, fmt.Errorf("prepare insert: %w", err)
	}
	defer stmt.Close()

	var jobs []*Job
	for _, uri := range uris {
		res, err := stmt.Exec(uri)
		if err != nil {
			return nil, fmt.Errorf("insert job for %q: %w", uri, err)
		}
		id, _ := res.LastInsertId()
		jobs = append(jobs, &Job{
			ID:         id,
			SpotifyURI: uri,
			Status:     StatusPending,
			CreatedAt:  time.Now(),
		})
	}

	if err := tx.Commit(); err != nil {
		return nil, fmt.Errorf("commit transaction: %w", err)
	}
	return jobs, nil
}

// Next claims and returns the next pending job, or nil if the queue is empty.
func (q *Queue) Next() (*Job, error) {
	tx, err := q.db.Begin()
	if err != nil {
		return nil, fmt.Errorf("begin transaction: %w", err)
	}
	defer tx.Rollback()

	row := tx.QueryRow(`
		SELECT id, spotify_uri, status, error, created_at, started_at, completed_at
		FROM jobs
		WHERE status = ?
		ORDER BY id ASC
		LIMIT 1
	`, StatusPending)

	job, err := scanJob(row)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("query next job: %w", err)
	}

	now := time.Now()
	if _, err := tx.Exec(
		"UPDATE jobs SET status = ?, started_at = ? WHERE id = ?",
		StatusRunning, now.Format(time.RFC3339), job.ID,
	); err != nil {
		return nil, fmt.Errorf("claim job %d: %w", job.ID, err)
	}

	if err := tx.Commit(); err != nil {
		return nil, fmt.Errorf("commit claim: %w", err)
	}

	job.Status = StatusRunning
	job.StartedAt = &now
	return job, nil
}

// Complete marks a job as done.
func (q *Queue) Complete(id int64) error {
	now := time.Now().Format(time.RFC3339)
	_, err := q.db.Exec(
		"UPDATE jobs SET status = ?, completed_at = ? WHERE id = ?",
		StatusDone, now, id,
	)
	if err != nil {
		return fmt.Errorf("complete job %d: %w", id, err)
	}
	return nil
}

// Fail marks a job as failed with an error message.
func (q *Queue) Fail(id int64, errMsg string) error {
	now := time.Now().Format(time.RFC3339)
	_, err := q.db.Exec(
		"UPDATE jobs SET status = ?, error = ?, completed_at = ? WHERE id = ?",
		StatusFailed, errMsg, now, id,
	)
	if err != nil {
		return fmt.Errorf("fail job %d: %w", id, err)
	}
	return nil
}

// List returns all jobs, most recent first.
func (q *Queue) List() ([]*Job, error) {
	rows, err := q.db.Query(`
		SELECT id, spotify_uri, status, error, created_at, started_at, completed_at
		FROM jobs
		ORDER BY id DESC
	`)
	if err != nil {
		return nil, fmt.Errorf("list jobs: %w", err)
	}
	defer rows.Close()

	var jobs []*Job
	for rows.Next() {
		job, err := scanJobRows(rows)
		if err != nil {
			return nil, fmt.Errorf("scan job: %w", err)
		}
		jobs = append(jobs, job)
	}
	return jobs, rows.Err()
}

// Close closes the underlying database.
func (q *Queue) Close() error {
	return q.db.Close()
}

func scanJob(row *sql.Row) (*Job, error) {
	var j Job
	var createdAt string
	var startedAt, completedAt sql.NullString

	if err := row.Scan(&j.ID, &j.SpotifyURI, &j.Status, &j.Error, &createdAt, &startedAt, &completedAt); err != nil {
		return nil, err
	}
	return parseJobTimes(&j, createdAt, startedAt, completedAt)
}

func scanJobRows(rows *sql.Rows) (*Job, error) {
	var j Job
	var createdAt string
	var startedAt, completedAt sql.NullString

	if err := rows.Scan(&j.ID, &j.SpotifyURI, &j.Status, &j.Error, &createdAt, &startedAt, &completedAt); err != nil {
		return nil, err
	}
	return parseJobTimes(&j, createdAt, startedAt, completedAt)
}

func parseJobTimes(j *Job, createdAt string, startedAt, completedAt sql.NullString) (*Job, error) {
	t, err := time.Parse(time.RFC3339, createdAt)
	if err != nil {
		// Try SQLite default format
		t, err = time.Parse("2006-01-02 15:04:05", createdAt)
		if err != nil {
			return nil, fmt.Errorf("parse created_at: %w", err)
		}
	}
	j.CreatedAt = t

	if startedAt.Valid {
		t, err := time.Parse(time.RFC3339, startedAt.String)
		if err == nil {
			j.StartedAt = &t
		}
	}
	if completedAt.Valid {
		t, err := time.Parse(time.RFC3339, completedAt.String)
		if err == nil {
			j.CompletedAt = &t
		}
	}
	return j, nil
}
