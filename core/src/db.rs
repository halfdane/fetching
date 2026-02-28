//! SQLite-backed persistent task database.
//!
//! Replaces the former sled-backed `TaskRegistry` with a normalised relational
//! schema: `collections`, `tracks`, `tasks`.
//!
//! # Usage
//!
//! ```ignore
//! let db = Arc::new(Database::open("fetching.db")?);
//! let entries = db.recover_interrupted()?;
//! let coord = Arc::new(DownloadCoordinator::with_db(db.clone(), apis, runner));
//! for entry in entries { coord.enqueue(entry); }
//! coord.start();
//! ```

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::container::{CollectionType, Track, TrackCollection};
use crate::coordinator::TrackInfo;
use crate::queue::TaskId;

// ---------------------------------------------------------------------------
// TaskStatus  (moved from registry.rs, wire-format unchanged)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Retrying,
    Done,
    /// `reason` is a human-readable error string.
    Failed { reason: String },
}

impl TaskStatus {
    /// Serialise for the `tasks.status` TEXT column.
    fn to_db(&self) -> String {
        match self {
            Self::Pending => "pending".into(),
            Self::Running => "running".into(),
            Self::Retrying => "retrying".into(),
            Self::Done => "done".into(),
            Self::Failed { reason } => format!("failed:{reason}"),
        }
    }

    /// Deserialise from the `tasks.status` TEXT column.
    fn from_db(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "retrying" => Self::Retrying,
            "done" => Self::Done,
            other => {
                if let Some(reason) = other.strip_prefix("failed:") {
                    Self::Failed {
                        reason: reason.to_owned(),
                    }
                } else {
                    warn!("Unknown task status in DB: {other}; treating as failed");
                    Self::Failed {
                        reason: format!("unknown status: {other}"),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Row types returned by queries
// ---------------------------------------------------------------------------

/// One row from `GET /api/collections` — collection with aggregated status/progress.
#[derive(Clone, Debug, Serialize)]
pub struct CollectionRow {
    pub id: String,
    pub uri: String,
    pub collection_type: String,
    pub title: String,
    pub artists: Vec<String>,
    pub cover_id: Option<String>,
    pub date: Option<String>,
    pub total_tracks: usize,
    /// Aggregated from task statuses.
    pub status: String,
    /// % of tasks that are done (0–100).
    pub progress: u32,
    pub registered_at: String,
}

/// One row from `GET /api/collections/{id}/tracks`.
#[derive(Clone, Debug, Serialize)]
pub struct TrackRow {
    pub id: String,
    pub uri: String,
    pub title: Option<String>,
    pub artists: Option<Vec<String>>,
    pub number: Option<i32>,
    pub disc_number: Option<i32>,
    pub duration_ms: Option<i32>,
    pub task_id: String,
    pub status: String,
    pub message: Option<String>,
}

/// Minimal data needed to re-queue an interrupted task.
#[derive(Clone, Debug)]
pub struct RecoveredEntry {
    pub task_id: TaskId,
    pub track_uri: String,
    pub collection: TrackCollection,
}

impl From<RecoveredEntry> for crate::queue::QueueEntry {
    fn from(r: RecoveredEntry) -> Self {
        Self {
            task_id: r.task_id,
            track_uri: r.track_uri,
            collection: std::sync::Arc::new(r.collection),
        }
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) a SQLite database at `path`.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (for tests).
    #[cfg(test)]
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS collections (
                id              TEXT PRIMARY KEY,
                uri             TEXT UNIQUE NOT NULL,
                collection_type TEXT NOT NULL,
                title           TEXT NOT NULL,
                artists         TEXT NOT NULL,   -- JSON array
                cover_id        TEXT,
                upc             TEXT,
                label           TEXT,
                date            TEXT,
                total_tracks    INTEGER NOT NULL,
                registered_at   TEXT NOT NULL     -- ISO 8601
            );

            CREATE TABLE IF NOT EXISTS tracks (
                id              TEXT PRIMARY KEY,
                uri             TEXT NOT NULL,
                collection_id   TEXT NOT NULL REFERENCES collections(id),
                title           TEXT,
                artists         TEXT,             -- JSON array, nullable
                cover_id        TEXT,
                isrc            TEXT,
                duration_ms     INTEGER,
                disc_number     INTEGER,
                number          INTEGER,
                date            TEXT,
                explicit        INTEGER NOT NULL DEFAULT 0,
                language        TEXT,             -- JSON array
                UNIQUE(uri, collection_id)
            );

            CREATE TABLE IF NOT EXISTS tasks (
                id              TEXT PRIMARY KEY,
                track_id        TEXT NOT NULL REFERENCES tracks(id),
                status          TEXT NOT NULL DEFAULT 'pending',
                message         TEXT,
                registered_at   TEXT NOT NULL     -- ISO 8601
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE INDEX IF NOT EXISTS idx_collections_registered_at ON collections(registered_at);
            CREATE INDEX IF NOT EXISTS idx_tracks_collection_id ON tracks(collection_id);
            CREATE INDEX IF NOT EXISTS idx_tracks_uri ON tracks(uri);
            ",
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Insert operations
    // -----------------------------------------------------------------------

    /// Insert a collection, returning its generated UUID.
    ///
    /// If a collection with the same `uri` already exists, returns its existing ID.
    pub fn insert_collection(&self, collection: &TrackCollection) -> anyhow::Result<String> {
        let conn = self.conn.lock().unwrap();

        // Check for existing collection with same URI
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM collections WHERE uri = ?1",
                params![collection.uri_str],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            // Re-add: bump registered_at so the collection appears newest-first.
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE collections SET registered_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        let artists_json = serde_json::to_string(&collection.artists)?;
        let collection_type = collection_type_to_str(&collection.collection_type);
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO collections (id, uri, collection_type, title, artists, cover_id, upc, label, date, total_tracks, registered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                collection.uri_str,
                collection_type,
                collection.title,
                artists_json,
                collection.cover_id,
                collection.upc,
                collection.label,
                collection.date,
                collection.total_tracks as i64,
                now,
            ],
        )?;

        Ok(id)
    }

    /// Insert a track (with just its URI — metadata nullable), returning its UUID.
    ///
    /// If a track with the same `(uri, collection_id)` already exists, returns its existing ID.
    pub fn insert_track(&self, track_uri: &str, collection_id: &str) -> anyhow::Result<String> {
        let conn = self.conn.lock().unwrap();

        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM tracks WHERE uri = ?1 AND collection_id = ?2",
                params![track_uri, collection_id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO tracks (id, uri, collection_id) VALUES (?1, ?2, ?3)",
            params![id, track_uri, collection_id],
        )?;

        Ok(id)
    }

    /// Insert a task for a track, returning its UUID (= the task_id).
    ///
    /// If a task already exists for this track, returns the existing task_id.
    pub fn insert_task(&self, track_id: &str) -> anyhow::Result<String> {
        let conn = self.conn.lock().unwrap();

        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM tasks WHERE track_id = ?1",
                params![track_id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            // Re-add: reset status so the track is re-downloaded.
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE tasks SET status = 'pending', message = NULL, registered_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO tasks (id, track_id, status, registered_at) VALUES (?1, ?2, 'pending', ?3)",
            params![id, track_id, now],
        )?;

        Ok(id)
    }

    // -----------------------------------------------------------------------
    // Update operations
    // -----------------------------------------------------------------------

    /// Update a task's status and optional message.
    pub fn update_task(
        &self,
        task_id: &str,
        status: &TaskStatus,
        message: Option<&str>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let status_str = status.to_db();
        conn.execute(
            "UPDATE tasks SET status = ?1, message = ?2 WHERE id = ?3",
            params![status_str, message, task_id],
        )?;
        Ok(())
    }

    /// Fill in the nullable metadata columns on a track row after the runner
    /// resolves track metadata from Spotify.
    pub fn update_track_metadata(&self, track_id: &str, track: &Track) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let artists_json = serde_json::to_string(&track.artists)?;
        let language_json = serde_json::to_string(&track.language)?;
        conn.execute(
            "UPDATE tracks SET
                title = ?1,
                artists = ?2,
                cover_id = ?3,
                isrc = ?4,
                duration_ms = ?5,
                disc_number = ?6,
                number = ?7,
                date = ?8,
                explicit = ?9,
                language = ?10
             WHERE id = ?11",
            params![
                track.title,
                artists_json,
                track.cover_id,
                track.isrc,
                track.duration_ms,
                track.disc_number,
                track.number,
                track.date,
                track.explicit as i32,
                language_json,
                track_id,
            ],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Query operations
    // -----------------------------------------------------------------------

    /// List all collections with aggregated task status and progress.
    /// Sorted by `registered_at` DESC (newest first).
    pub fn list_collections(&self) -> anyhow::Result<Vec<CollectionRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT
                c.id,
                c.uri,
                c.collection_type,
                c.title,
                c.artists,
                c.cover_id,
                c.date,
                c.total_tracks,
                c.registered_at,
                COUNT(t2.id) AS task_count,
                COALESCE(SUM(CASE WHEN t2.status = 'done' THEN 1 ELSE 0 END), 0) AS done_count,
                COALESCE(SUM(CASE WHEN t2.status IN ('running', 'retrying') THEN 1 ELSE 0 END), 0) AS active_count,
                COALESCE(SUM(CASE WHEN t2.status LIKE 'failed:%' THEN 1 ELSE 0 END), 0) AS failed_count
             FROM collections c
             LEFT JOIN tracks tr ON tr.collection_id = c.id
             LEFT JOIN tasks t2 ON t2.track_id = tr.id
             GROUP BY c.id
             ORDER BY c.registered_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let task_count: i64 = row.get(9)?;
            let done_count: i64 = row.get(10)?;
            let active_count: i64 = row.get(11)?;
            let failed_count: i64 = row.get(12)?;

            let status = if active_count > 0 {
                "running"
            } else if task_count > 0 && done_count == task_count {
                "done"
            } else if failed_count > 0 {
                "failed"
            } else {
                "pending"
            };

            let progress = if task_count > 0 {
                ((done_count * 100) / task_count) as u32
            } else {
                0
            };

            let artists_json: String = row.get(4)?;
            let artists: Vec<String> =
                serde_json::from_str(&artists_json).unwrap_or_default();

            Ok(CollectionRow {
                id: row.get(0)?,
                uri: row.get(1)?,
                collection_type: row.get(2)?,
                title: row.get(3)?,
                artists,
                cover_id: row.get(5)?,
                date: row.get(6)?,
                total_tracks: row.get::<_, i64>(7)? as usize,
                status: status.to_string(),
                progress,
                registered_at: row.get(8)?,
            })
        })?;

        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Fetch all tracks (with task status) for a given collection.
    /// Ordered by track number, then disc number.
    pub fn get_tracks_for_collection(&self, collection_id: &str) -> anyhow::Result<Vec<TrackRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT
                tr.id,
                tr.uri,
                tr.title,
                tr.artists,
                tr.number,
                tr.disc_number,
                tr.duration_ms,
                t.id AS task_id,
                t.status,
                t.message
             FROM tracks tr
             JOIN tasks t ON t.track_id = tr.id
             WHERE tr.collection_id = ?1
             ORDER BY COALESCE(tr.disc_number, 0), COALESCE(tr.number, 0)",
        )?;

        let rows = stmt.query_map(params![collection_id], |row| {
            let artists_json: Option<String> = row.get(3)?;
            let artists: Option<Vec<String>> = artists_json
                .as_deref()
                .and_then(|j| serde_json::from_str(j).ok());

            let status_str: String = row.get(8)?;

            Ok(TrackRow {
                id: row.get(0)?,
                uri: row.get(1)?,
                title: row.get(2)?,
                artists,
                number: row.get(4)?,
                disc_number: row.get(5)?,
                duration_ms: row.get(6)?,
                task_id: row.get(7)?,
                status: TaskStatus::from_db(&status_str).to_db(),
                message: row.get(9)?,
            })
        })?;

        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Look up which collection a task belongs to.
    /// Returns `(collection_id, track_uri)` or `None`.
    pub fn collection_for_task(&self, task_id: &str) -> anyhow::Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT tr.collection_id, tr.uri
             FROM tasks t
             JOIN tracks tr ON tr.id = t.track_id
             WHERE t.id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(Into::into)
    }

    /// Return the track_id for a given task_id.
    pub fn track_id_for_task(&self, task_id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT track_id FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Collection finalization helpers
    // -----------------------------------------------------------------------

    /// Returns `true` when every task for a collection has status `done` or `failed:*`.
    pub fn is_collection_complete(&self, collection_id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let pending: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM tasks t
             JOIN tracks tr ON tr.id = t.track_id
             WHERE tr.collection_id = ?1
               AND t.status NOT IN ('done')
               AND t.status NOT LIKE 'failed:%'",
            params![collection_id],
            |row| row.get(0),
        )?;
        Ok(pending == 0)
    }

    /// Get the full TrackCollection for a collection_id (needed for finalization).
    pub fn get_collection(&self, collection_id: &str) -> anyhow::Result<Option<TrackCollection>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT uri, collection_type, title, artists, cover_id, upc, label, date, total_tracks
                 FROM collections WHERE id = ?1",
                params![collection_id],
                |row| {
                    let artists_json: String = row.get(3)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        artists_json,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, i64>(8)?,
                    ))
                },
            )
            .optional()?;

        let Some((uri, ctype, title, artists_json, cover_id, upc, label, date, total_tracks)) = row
        else {
            return Ok(None);
        };

        let artists: Vec<String> = serde_json::from_str(&artists_json).unwrap_or_default();
        let collection_type = str_to_collection_type(&ctype);

        // Fetch track URIs in order
        let mut stmt = conn.prepare(
            "SELECT uri FROM tracks WHERE collection_id = ?1 ORDER BY COALESCE(disc_number, 0), COALESCE(number, 0)",
        )?;
        let track_uris: Vec<String> = stmt
            .query_map(params![collection_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Some(TrackCollection {
            uri_str: uri,
            collection_type,
            title,
            artists,
            cover_id,
            upc,
            total_tracks: total_tracks as usize,
            label,
            date,
            track_uris,
        }))
    }

    /// Get TrackInfo for a track by its ID (used for SSE enrichment).
    pub fn get_track_info(&self, track_id: &str) -> anyhow::Result<Option<TrackInfo>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT title, artists, number, disc_number, duration_ms
             FROM tracks WHERE id = ?1 AND title IS NOT NULL",
            params![track_id],
            |row| {
                let artists_json: Option<String> = row.get(1)?;
                let artists: Vec<String> = artists_json
                    .as_deref()
                    .and_then(|j| serde_json::from_str(j).ok())
                    .unwrap_or_default();
                Ok(TrackInfo {
                    title: row.get(0)?,
                    artists,
                    number: row.get(2)?,
                    disc_number: row.get(3)?,
                    duration_ms: row.get::<_, i32>(4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Recovery
    // -----------------------------------------------------------------------

    /// Reset interrupted tasks and return entries that need re-queuing.
    ///
    /// - `Running` / `Retrying` → reset to `pending`
    /// - `Pending` → returned as-is
    ///
    /// The caller pushes the returned entries into the coordinator's in-memory queue.
    pub fn recover_interrupted(&self) -> anyhow::Result<Vec<RecoveredEntry>> {
        let conn = self.conn.lock().unwrap();

        // Reset running/retrying to pending
        conn.execute(
            "UPDATE tasks SET status = 'pending', message = NULL
             WHERE status IN ('running', 'retrying')",
            [],
        )?;

        // Fetch all pending tasks with their track URIs and collection data
        let mut stmt = conn.prepare(
            "SELECT t.id, tr.uri, tr.collection_id
             FROM tasks t
             JOIN tracks tr ON tr.id = t.track_id
             WHERE t.status = 'pending'
             ORDER BY t.registered_at ASC",
        )?;

        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .filter_map(|r| r.ok())
            .collect();

        // We need to drop `conn` before calling `get_collection` which re-locks
        drop(stmt);
        drop(conn);

        let mut entries = Vec::new();
        for (task_id_str, track_uri, collection_id) in rows {
            let task_id = Uuid::parse_str(&task_id_str)?;
            if let Some(collection) = self.get_collection(&collection_id)? {
                entries.push(RecoveredEntry {
                    task_id,
                    track_uri,
                    collection,
                });
            } else {
                warn!(
                    "Collection {collection_id} not found for task {task_id_str} during recovery; skipping"
                );
            }
        }

        if !entries.is_empty() {
            info!(
                "Recovered {} interrupted/pending task(s) from previous session",
                entries.len()
            );
        }

        Ok(entries)
    }

    // -----------------------------------------------------------------------
    // Convenience: insert a full collection with all its tracks and tasks
    // -----------------------------------------------------------------------

    /// Insert a collection, all its tracks, and a task per track.
    /// Returns `(collection_id, Vec<(track_id, task_id)>)`.
    ///
    /// Idempotent: re-inserting the same collection URI reuses existing rows.
    pub fn insert_collection_with_tracks(
        &self,
        collection: &TrackCollection,
    ) -> anyhow::Result<(String, Vec<(String, String)>)> {
        let collection_id = self.insert_collection(collection)?;

        let mut id_pairs = Vec::with_capacity(collection.track_uris.len());
        for track_uri in &collection.track_uris {
            let track_id = self.insert_track(track_uri, &collection_id)?;
            let task_id = self.insert_task(&track_id)?;
            id_pairs.push((track_id, task_id));
        }

        Ok((collection_id, id_pairs))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collection_type_to_str(ct: &CollectionType) -> &'static str {
    match ct {
        CollectionType::Album => "album",
        CollectionType::Playlist => "playlist",
        CollectionType::Show => "show",
        CollectionType::SingleTrack => "single_track",
        CollectionType::SingleEpisode => "single_episode",
    }
}

fn str_to_collection_type(s: &str) -> CollectionType {
    match s {
        "album" => CollectionType::Album,
        "playlist" => CollectionType::Playlist,
        "show" => CollectionType::Show,
        "single_track" => CollectionType::SingleTrack,
        "single_episode" => CollectionType::SingleEpisode,
        _ => {
            warn!("Unknown collection_type in DB: {s}; defaulting to Album");
            CollectionType::Album
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{CollectionType, Track, TrackCollection};

    fn fake_collection(uris: &[&str]) -> TrackCollection {
        TrackCollection {
            uri_str: "spotify:album:test".to_string(),
            collection_type: CollectionType::Album,
            title: "Test Album".to_string(),
            artists: vec!["Artist".to_string()],
            cover_id: Some("cover123".to_string()),
            upc: Some("UPC123".to_string()),
            total_tracks: uris.len(),
            label: Some("Test Label".to_string()),
            date: Some("2024-01-01".to_string()),
            track_uris: uris.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn fake_track(uri: &str) -> Track {
        Track {
            uri_str: uri.to_string(),
            title: "Test Track".to_string(),
            artists: vec!["Track Artist".to_string()],
            cover_id: Some("track_cover".to_string()),
            isrc: Some("ISRC123".to_string()),
            duration_ms: 180_000,
            disc_number: Some(1),
            number: Some(3),
            date: Some("2024-01-01".to_string()),
            explicit: false,
            language: vec!["en".to_string()],
        }
    }

    // -- insert & list round-trip --

    #[test]
    fn insert_collection_with_tracks_and_list() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (coll_id, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        assert_eq!(pairs.len(), 2);

        let collections = db.list_collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].id, coll_id);
        assert_eq!(collections[0].title, "Test Album");
        assert_eq!(collections[0].status, "pending");
        assert_eq!(collections[0].progress, 0);
    }

    // -- idempotent insert --

    #[test]
    fn insert_same_collection_twice_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a"]);
        let (id1, _) = db.insert_collection_with_tracks(&col).unwrap();
        let (id2, _) = db.insert_collection_with_tracks(&col).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(db.list_collections().unwrap().len(), 1);
    }

    // -- update task status --

    #[test]
    fn update_task_changes_status() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a"]);
        let (_, pairs) = db.insert_collection_with_tracks(&col).unwrap();
        let task_id = &pairs[0].1;

        db.update_task(task_id, &TaskStatus::Running, Some("Running…"))
            .unwrap();

        let collections = db.list_collections().unwrap();
        assert_eq!(collections[0].status, "running");
    }

    // -- update track metadata --

    #[test]
    fn update_track_metadata_fills_nulls() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a"]);
        let (coll_id, pairs) = db.insert_collection_with_tracks(&col).unwrap();
        let track_id = &pairs[0].0;

        // Before update: title should be null
        let tracks = db.get_tracks_for_collection(&coll_id).unwrap();
        assert!(tracks[0].title.is_none());

        // Update metadata
        db.update_track_metadata(track_id, &fake_track("uri:a")).unwrap();

        let tracks = db.get_tracks_for_collection(&coll_id).unwrap();
        assert_eq!(tracks[0].title.as_deref(), Some("Test Track"));
        assert_eq!(tracks[0].duration_ms, Some(180_000));
    }

    // -- aggregated status --

    #[test]
    fn aggregated_status_done_when_all_done() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (_, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        for (_, task_id) in &pairs {
            db.update_task(task_id, &TaskStatus::Done, Some("Downloaded"))
                .unwrap();
        }

        let collections = db.list_collections().unwrap();
        assert_eq!(collections[0].status, "done");
        assert_eq!(collections[0].progress, 100);
    }

    #[test]
    fn aggregated_status_running_when_any_active() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (_, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        db.update_task(&pairs[0].1, &TaskStatus::Done, None).unwrap();
        db.update_task(&pairs[1].1, &TaskStatus::Running, None).unwrap();

        let collections = db.list_collections().unwrap();
        assert_eq!(collections[0].status, "running");
        assert_eq!(collections[0].progress, 50);
    }

    #[test]
    fn aggregated_status_failed_when_any_failed_none_active() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (_, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        db.update_task(&pairs[0].1, &TaskStatus::Done, None).unwrap();
        db.update_task(
            &pairs[1].1,
            &TaskStatus::Failed { reason: "oops".into() },
            None,
        )
        .unwrap();

        let collections = db.list_collections().unwrap();
        assert_eq!(collections[0].status, "failed");
    }

    // -- collection_for_task --

    #[test]
    fn collection_for_task_returns_correct_ids() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a"]);
        let (coll_id, pairs) = db.insert_collection_with_tracks(&col).unwrap();
        let task_id = &pairs[0].1;

        let (found_coll, found_uri) = db.collection_for_task(task_id).unwrap().unwrap();
        assert_eq!(found_coll, coll_id);
        assert_eq!(found_uri, "uri:a");
    }

    // -- is_collection_complete --

    #[test]
    fn is_collection_complete_false_when_pending() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (coll_id, _) = db.insert_collection_with_tracks(&col).unwrap();
        assert!(!db.is_collection_complete(&coll_id).unwrap());
    }

    #[test]
    fn is_collection_complete_true_when_all_done() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (coll_id, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        for (_, task_id) in &pairs {
            db.update_task(task_id, &TaskStatus::Done, None).unwrap();
        }
        assert!(db.is_collection_complete(&coll_id).unwrap());
    }

    #[test]
    fn is_collection_complete_true_when_mix_of_done_and_failed() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (coll_id, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        db.update_task(&pairs[0].1, &TaskStatus::Done, None).unwrap();
        db.update_task(
            &pairs[1].1,
            &TaskStatus::Failed { reason: "err".into() },
            None,
        )
        .unwrap();
        assert!(db.is_collection_complete(&coll_id).unwrap());
    }

    // -- recover_interrupted --

    #[test]
    fn recover_interrupted_resets_running_to_pending() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a"]);
        let (_, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        db.update_task(&pairs[0].1, &TaskStatus::Running, Some("Running…"))
            .unwrap();

        let recovered = db.recover_interrupted().unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].track_uri, "uri:a");
    }

    #[test]
    fn recover_interrupted_returns_pending_entries() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a"]);
        let _ = db.insert_collection_with_tracks(&col).unwrap();

        let recovered = db.recover_interrupted().unwrap();
        assert_eq!(recovered.len(), 1);
    }

    #[test]
    fn recover_interrupted_skips_done_and_failed() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (_, pairs) = db.insert_collection_with_tracks(&col).unwrap();

        db.update_task(&pairs[0].1, &TaskStatus::Done, None).unwrap();
        db.update_task(
            &pairs[1].1,
            &TaskStatus::Failed { reason: "oops".into() },
            None,
        )
        .unwrap();

        let recovered = db.recover_interrupted().unwrap();
        assert!(recovered.is_empty());
    }

    // -- get_collection round-trip --

    #[test]
    fn get_collection_returns_full_data() {
        let db = Database::open_in_memory().unwrap();
        let col = fake_collection(&["uri:a", "uri:b"]);
        let (coll_id, _) = db.insert_collection_with_tracks(&col).unwrap();

        let loaded = db.get_collection(&coll_id).unwrap().unwrap();
        assert_eq!(loaded.title, "Test Album");
        assert_eq!(loaded.artists, vec!["Artist".to_string()]);
        assert_eq!(loaded.track_uris.len(), 2);
    }

    // -- TaskStatus serde (wire format) --

    #[test]
    fn task_status_serde_round_trips_all_variants() {
        let cases = [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Done,
            TaskStatus::Failed {
                reason: "oops".to_string(),
            },
        ];
        for status in cases {
            let json = serde_json::to_string(&status).unwrap();
            let back: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status, "round-trip failed for {json}");
        }
    }

    #[test]
    fn task_status_db_round_trips() {
        let cases = [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Retrying,
            TaskStatus::Done,
            TaskStatus::Failed {
                reason: "disk full".to_string(),
            },
        ];
        for status in &cases {
            let db_str = status.to_db();
            let back = TaskStatus::from_db(&db_str);
            assert_eq!(&back, status, "DB round-trip failed for {db_str}");
        }
    }
}
