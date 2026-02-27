//! Sled-backed persistent task registry.
//!
//! Tracks every task's status across its full lifecycle so that
//! `GET /api/queue` can return a complete snapshot even after page refresh.
//!
//! The FIFO download queue itself is in-memory ([`TrackQueue`](crate::queue_memory::TrackQueue));
//! sled only stores the registry.  On startup, [`SledRegistry::recover_interrupted`]
//! returns any tasks that need re-queuing so the coordinator can push them.
//!
//! # Usage
//!
//! ```ignore
//! let registry = Arc::new(SledRegistry::open("queue.sled")?);
//! let coord = Arc::new(DownloadCoordinator::with_registry(registry.clone(), apis, runner));
//! for entry in registry.recover_interrupted()? {
//!     coord.enqueue(entry);
//! }
//! coord.start();
//! ```

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::container::TrackCollection;
use crate::coordinator::{ProgressUpdate, TrackInfo};
use crate::queue::{QueueEntry, TaskId};

// ---------------------------------------------------------------------------
// Task status
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

// ---------------------------------------------------------------------------
// TaskSnapshot
// ---------------------------------------------------------------------------

/// Point-in-time view of a single queued task, as returned by
/// [`TaskRegistry::snapshot`].
#[derive(Clone, Debug)]
pub struct TaskSnapshot {
    pub task_id: TaskId,
    pub track_uri: String,
    pub collection: Arc<TrackCollection>,
    pub status: TaskStatus,
    pub message: Option<String>,
    pub track_info: Option<TrackInfo>,
    /// Unix epoch milliseconds at which this task was first registered.
    /// Used to preserve stable enqueue order across restarts.
    pub registered_at: u64,
}

// ---------------------------------------------------------------------------
// TaskRegistry trait
// ---------------------------------------------------------------------------

/// Persists every task's status across its full lifecycle:
/// `Pending → Running → Done / Failed`.
///
/// # Integration points
///
/// - [`DownloadCoordinator::add_collection`](crate::coordinator::DownloadCoordinator::add_collection)
///   calls `register` for each new entry.
/// - The worker loop calls `update` (via `emit_update`) at every status
///   transition alongside the SSE broadcast — no separate background task
///   needed.
pub trait TaskRegistry: Send + Sync + 'static {
    /// Record a new task with its initial status (`Pending`) at enqueue time.
    ///
    /// Implementations must replace any existing entry for the same `track_uri`,
    /// so the registry always holds exactly one record per track URI.
    fn register(&self, entry: &QueueEntry, status: TaskStatus) -> anyhow::Result<()>;

    /// Update a task from a progress update (status, message, track_info).
    fn update(&self, update: &ProgressUpdate) -> anyhow::Result<()>;

    /// Return all known tasks with their current status.
    fn snapshot(&self) -> anyhow::Result<Vec<TaskSnapshot>>;
}

/// Full task record stored in the sled `tasks` tree.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredTask {
    task_id: TaskId,
    track_uri: String,
    collection: TrackCollection,
    status: TaskStatus,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    track_info: Option<TrackInfo>,
    /// Unix epoch milliseconds at first registration. `0` for records
    /// persisted before this field was added (treated as oldest).
    #[serde(default)]
    registered_at: u64,
}

/// Sled-backed persistent task registry.
///
/// Stores task status in a single sled tree keyed by `task_id` bytes.
///
/// ```ignore
/// let registry = Arc::new(SledRegistry::open("queue.sled")?);
/// let coord = Arc::new(DownloadCoordinator::with_registry(registry.clone(), apis, runner));
/// ```
pub struct SledRegistry {
    tasks: sled::Tree,
}

const MAX_REGISTRY_ENTRIES: usize = 1_000;

impl SledRegistry {
    /// Open (or create) a sled database at `path`.
    pub fn open(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let db = sled::open(path)?;
        let tasks = db.open_tree("tasks")?;
        Ok(Self { tasks })
    }

    /// Return entries that need re-queuing after a crash or restart.
    ///
    /// - `Running` tasks are reset to `Pending` (they were mid-flight when
    ///   the server went down).
    /// - `Pending` / `Retrying` tasks are returned as-is (they were waiting
    ///   in the in-memory queue that was lost).
    ///
    /// The caller (typically `main.rs`) pushes the returned entries into the
    /// coordinator's in-memory queue.
    pub fn recover_interrupted(&self) -> anyhow::Result<Vec<QueueEntry>> {
        let mut entries = Vec::new();
        for result in self.tasks.iter() {
            let (key, value) = result?;
            let mut stored: StoredTask = match serde_json::from_slice(&value) {
                Ok(t) => t,
                Err(e) => {
                    warn!("Skipping corrupt task entry during recovery: {e}");
                    self.tasks.remove(&key)?;
                    continue;
                }
            };
            match stored.status {
                TaskStatus::Running => {
                    stored.status = TaskStatus::Pending;
                    self.tasks.insert(&key, serde_json::to_vec(&stored)?)?;
                    entries.push(QueueEntry {
                        task_id: stored.task_id,
                        track_uri: stored.track_uri,
                        collection: Arc::new(stored.collection),
                    });
                }
                TaskStatus::Pending | TaskStatus::Retrying => {
                    entries.push(QueueEntry {
                        task_id: stored.task_id,
                        track_uri: stored.track_uri,
                        collection: Arc::new(stored.collection),
                    });
                }
                TaskStatus::Done | TaskStatus::Failed { .. } => {}
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

    fn prune_to_limit(&self) -> anyhow::Result<()> {
        self.prune_to(MAX_REGISTRY_ENTRIES)
    }

    fn prune_to(&self, limit: usize) -> anyhow::Result<()> {
        let total = self.tasks.len();
        if total <= limit {
            return Ok(());
        }
        // Only finished tasks are safe to drop; active ones must stay.
        let mut finished: Vec<(u64, sled::IVec)> = self
            .tasks
            .iter()
            .filter_map(|res| {
                let (key, value) = res.ok()?;
                let stored: StoredTask = serde_json::from_slice(&value).ok()?;
                matches!(stored.status, TaskStatus::Done | TaskStatus::Failed { .. })
                    .then_some((stored.registered_at, key))
            })
            .collect();
        finished.sort_unstable_by_key(|(ts, _)| *ts);
        let to_remove = total - limit;
        for (_, key) in finished.into_iter().take(to_remove) {
            self.tasks.remove(key)?;
        }
        if to_remove > 0 {
            info!("Pruned {to_remove} old finished task(s) from registry (limit {limit})");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TaskRegistry impl
// ---------------------------------------------------------------------------

impl TaskRegistry for SledRegistry {
    fn register(&self, entry: &QueueEntry, status: TaskStatus) -> anyhow::Result<()> {
        // Enforce one-entry-per-track-uri: remove any previous record for this
        // URI before inserting the new one (handles retries and re-queues).
        let stale_keys: Vec<_> = self
            .tasks
            .iter()
            .filter_map(|res| {
                let (key, value) = res.ok()?;
                let stored: StoredTask = serde_json::from_slice(&value).ok()?;
                if stored.track_uri == entry.track_uri {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();
        for key in stale_keys {
            self.tasks.remove(key)?;
        }

        let registered_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let stored = StoredTask {
            task_id: entry.task_id,
            track_uri: entry.track_uri.clone(),
            collection: (*entry.collection).clone(),
            status,
            message: None,
            track_info: None,
            registered_at,
        };
        self.tasks
            .insert(entry.task_id.as_bytes(), serde_json::to_vec(&stored)?)?;
        self.prune_to_limit()?;
        Ok(())
    }

    fn update(&self, update: &ProgressUpdate) -> anyhow::Result<()> {
        let key = update.task_id.as_bytes();
        let Some(bytes) = self.tasks.get(key)? else {
            warn!("TaskRegistry::update called for unknown task {}", update.task_id);
            return Ok(());
        };
        let mut stored: StoredTask = serde_json::from_slice(&bytes)?;
        stored.status = update.status.clone();
        if update.message.is_some() {
            stored.message = update.message.clone();
        }
        if update.track_info.is_some() {
            stored.track_info = update.track_info.clone();
        }
        self.tasks.insert(key, serde_json::to_vec(&stored)?)?;
        Ok(())
    }

    fn snapshot(&self) -> anyhow::Result<Vec<TaskSnapshot>> {
        self.tasks
            .iter()
            .map(|res| {
                let (_, value) = res?;
                let stored: StoredTask = serde_json::from_slice(&value)?;
                Ok(TaskSnapshot {
                    task_id: stored.task_id,
                    track_uri: stored.track_uri,
                    collection: Arc::new(stored.collection),
                    status: stored.status,
                    message: stored.message,
                    track_info: stored.track_info,
                    registered_at: stored.registered_at,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{CollectionType, TrackCollection};

    // -- TaskStatus serde contract (wire format shared with the frontend) ----

    #[test]
    fn task_status_serde_round_trips_all_variants() {
        let cases = [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Done,
            TaskStatus::Failed { reason: "oops".to_string() },
        ];
        for status in cases {
            let json = serde_json::to_string(&status).unwrap();
            let back: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status, "round-trip failed for {json}");
        }
    }

    #[test]
    fn task_status_failed_includes_reason_in_json() {
        let status = TaskStatus::Failed { reason: "disk full".to_string() };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("disk full"), "reason missing from: {json}");
    }

    fn fake_collection(uris: &[&str]) -> Arc<TrackCollection> {
        Arc::new(TrackCollection {
            uri_str: "spotify:album:test".to_string(),
            collection_type: CollectionType::Album,
            title: "Test Album".to_string(),
            artists: vec!["Artist".to_string()],
            cover_id: None,
            upc: None,
            total_tracks: uris.len(),
            label: None,
            date: None,
            track_uris: uris.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Open a temporary sled-backed registry that is cleaned up on drop.
    fn temp_registry() -> SledRegistry {
        let dir = tempfile::tempdir().unwrap();
        SledRegistry::open(dir.path().join("test.sled")).unwrap()
    }

    // -- register + snapshot ------------------------------------------------

    #[test]
    fn register_and_snapshot_round_trips() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a"]);
        let entry = QueueEntry::new("uri:a", col);
        reg.register(&entry, TaskStatus::Pending).unwrap();

        let snap = reg.snapshot().unwrap();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].task_id, entry.task_id);
        assert_eq!(snap[0].track_uri, "uri:a");
        assert_eq!(snap[0].status, TaskStatus::Pending);
    }

    #[test]
    fn snapshot_on_empty_registry_returns_empty_vec() {
        let reg = temp_registry();
        assert!(reg.snapshot().unwrap().is_empty());
    }

    // -- dedup invariant ----------------------------------------------------

    #[test]
    fn register_replaces_existing_entry_for_same_track_uri() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a"]);

        let first = QueueEntry::new("uri:a", Arc::clone(&col));
        reg.register(&first, TaskStatus::Done).unwrap();

        let second = QueueEntry::new("uri:a", col);
        reg.register(&second, TaskStatus::Pending).unwrap();

        let snap = reg.snapshot().unwrap();
        assert_eq!(snap.len(), 1, "old entry should have been replaced");
        assert_eq!(snap[0].task_id, second.task_id);
        assert_eq!(snap[0].status, TaskStatus::Pending);
    }

    #[test]
    fn register_does_not_replace_different_track_uri() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a", "uri:b"]);

        let a = QueueEntry::new("uri:a", Arc::clone(&col));
        let b = QueueEntry::new("uri:b", col);
        reg.register(&a, TaskStatus::Pending).unwrap();
        reg.register(&b, TaskStatus::Pending).unwrap();

        assert_eq!(reg.snapshot().unwrap().len(), 2);
    }

    // -- update -------------------------------------------------------------

    #[test]
    fn update_changes_status() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a"]);
        let entry = QueueEntry::new("uri:a", col);
        reg.register(&entry, TaskStatus::Pending).unwrap();

        reg.update(&ProgressUpdate {
            task_id: entry.task_id,
            status: TaskStatus::Running,
            message: Some("Running…".into()),
            track_info: None,
        }).unwrap();

        let snap = reg.snapshot().unwrap();
        assert_eq!(snap[0].status, TaskStatus::Running);
        assert_eq!(snap[0].message.as_deref(), Some("Running…"));
    }

    #[test]
    fn update_unknown_task_is_noop() {
        let reg = temp_registry();
        // Should not panic or error — just warn and return Ok.
        reg.update(&ProgressUpdate {
            task_id: TaskId::new_v4(),
            status: TaskStatus::Done,
            message: None,
            track_info: None,
        }).unwrap();
    }

    // -- recover_interrupted ------------------------------------------------

    #[test]
    fn recover_returns_running_as_pending() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a"]);
        let entry = QueueEntry::new("uri:a", Arc::clone(&col));
        reg.register(&entry, TaskStatus::Running).unwrap();

        let recovered = reg.recover_interrupted().unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].task_id, entry.task_id);

        // Status on disk should now be Pending.
        let snap = reg.snapshot().unwrap();
        assert_eq!(snap[0].status, TaskStatus::Pending);
    }

    #[test]
    fn recover_returns_pending_entries() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a"]);
        let entry = QueueEntry::new("uri:a", col);
        reg.register(&entry, TaskStatus::Pending).unwrap();

        let recovered = reg.recover_interrupted().unwrap();
        assert_eq!(recovered.len(), 1);
    }

    #[test]
    fn recover_skips_done_and_failed() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a", "uri:b"]);

        let done = QueueEntry::new("uri:a", Arc::clone(&col));
        reg.register(&done, TaskStatus::Done).unwrap();

        let failed = QueueEntry::new("uri:b", col);
        reg.register(
            &failed,
            TaskStatus::Failed {
                reason: "oops".into(),
            },
        )
        .unwrap();

        let recovered = reg.recover_interrupted().unwrap();
        assert!(recovered.is_empty(), "Done/Failed should not be recovered");
    }

    #[test]
    fn recover_on_empty_registry_returns_empty() {
        let reg = temp_registry();
        assert!(reg.recover_interrupted().unwrap().is_empty());
    }

    // -- prune_to -----------------------------------------------------------

    #[test]
    fn prune_does_nothing_at_or_below_limit() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a", "uri:b"]);
        for uri in ["uri:a", "uri:b"] {
            reg.register(&QueueEntry::new(uri, Arc::clone(&col)), TaskStatus::Done)
                .unwrap();
        }
        reg.prune_to(2).unwrap();
        assert_eq!(reg.snapshot().unwrap().len(), 2);
    }

    #[test]
    fn prune_drops_oldest_finished_tasks_first() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a", "uri:b", "uri:c"]);

        // Register with small sleeps so registered_at differs.
        let entries: Vec<_> = ["uri:a", "uri:b", "uri:c"]
            .iter()
            .map(|uri| {
                let e = QueueEntry::new(*uri, Arc::clone(&col));
                reg.register(&e, TaskStatus::Done).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(2));
                e
            })
            .collect();

        // Limit to 2 — should drop the oldest (uri:a).
        reg.prune_to(2).unwrap();

        let snap = reg.snapshot().unwrap();
        assert_eq!(snap.len(), 2, "should have pruned down to limit");
        let remaining_uris: Vec<_> = snap.iter().map(|s| s.track_uri.as_str()).collect();
        assert!(!remaining_uris.contains(&"uri:a"), "oldest entry should have been pruned");
        assert!(remaining_uris.contains(&"uri:b"));
        assert!(remaining_uris.contains(&"uri:c"));
        let _ = entries; // keep alive
    }

    #[test]
    fn prune_never_removes_active_tasks() {
        let reg = temp_registry();
        let col = fake_collection(&["uri:a", "uri:b", "uri:c"]);

        reg.register(&QueueEntry::new("uri:a", Arc::clone(&col)), TaskStatus::Done)
            .unwrap();
        reg.register(&QueueEntry::new("uri:b", Arc::clone(&col)), TaskStatus::Pending)
            .unwrap();
        reg.register(&QueueEntry::new("uri:c", Arc::clone(&col)), TaskStatus::Running)
            .unwrap();

        // Limit of 1 — can only prune finished tasks, so active two must survive.
        reg.prune_to(1).unwrap();

        let snap = reg.snapshot().unwrap();
        assert_eq!(snap.len(), 2, "only Done task should have been removed");
        let uris: Vec<_> = snap.iter().map(|s| s.track_uri.as_str()).collect();
        assert!(uris.contains(&"uri:b"), "Pending task must not be pruned");
        assert!(uris.contains(&"uri:c"), "Running task must not be pruned");
    }
}
