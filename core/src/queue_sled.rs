//! Sled-backed `QueueStorage` implementation.
//!
//! Provides durable FIFO storage so pending entries survive server restarts.
//! Behaviour is otherwise identical to `InMemoryStorage` – pop removes the
//! oldest entry first (smallest auto-generated key).
//!
//! # Usage
//!
//! ```ignore
//! let queue = TokioQueue::with_storage(
//!     SledStorage::open("queue.sled")?,
//!     apis,
//!     runner,
//! );
//! ```
//!
//! To revert to in-memory storage, replace the line above with:
//! ```ignore
//! let queue = TokioQueue::new(apis, runner);
//! ```

use std::path::Path;
use std::sync::Arc;

use sled::Db;
use tracing::{info, warn};

use crate::container::TrackCollection;
use crate::queue::{QueueEntry, QueueStorage, TaskId, TaskRegistry, TaskSnapshot, TaskStatus};

/// Owned, serialisable mirror of `QueueEntry`.
/// `QueueEntry` uses `Arc<TrackCollection>` for zero-copy sharing in memory;
/// on disk we store plain owned values and reconstruct the `Arc` on load.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredEntry {
    task_id: TaskId,
    track_uri: String,
    collection: TrackCollection,
}

/// Full task record stored in the `tasks` sled tree.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredTask {
    task_id: TaskId,
    track_uri: String,
    collection: TrackCollection,
    status: TaskStatus,
}

/// Sled-backed FIFO queue storage *and* task registry.
///
/// Uses two sled trees:
/// - **default tree** — the FIFO pending queue (auto-increment `u64` keys)
/// - **`tasks` tree** — full status history keyed by `task_id` bytes
///
/// Because both traits are implemented on the same struct, callers that want
/// both capabilities wrap it in an `Arc` and pass it twice:
///
/// ```ignore
/// let sled = Arc::new(SledStorage::open("queue.sled")?);
/// let queue = TokioQueue::with_registry(
///     sled.clone(), sled.clone(), apis, runner,
/// );
/// ```
pub struct SledStorage {
    db: Db,
    tasks: sled::Tree,
}

impl SledStorage {
    /// Open (or create) a sled database at `path`.
    ///
    /// On open, any task that was `Running` at shutdown is reset to `Pending`
    /// and re-pushed to the FIFO queue so the worker picks it up again.
    /// This provides crash-recovery semantics at the cost of re-downloading
    /// a partially-complete track.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let db = sled::open(path)?;
        let tasks = db.open_tree("tasks")?;
        let storage = Self { db, tasks };
        storage.recover_interrupted()?;
        Ok(storage)
    }

    /// Re-queue any tasks that were `Running` when the server last shut down.
    ///
    /// Resets their status to `Pending` and pushes them back onto the FIFO queue
    /// so the worker picks them up again on startup.  Done/Failed entries are
    /// left untouched — retention/cleanup is a separate concern handled elsewhere.
    fn recover_interrupted(&self) -> anyhow::Result<()> {
        let mut recovered = 0usize;
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
            if stored.status == TaskStatus::Running {
                stored.status = TaskStatus::Pending;
                self.tasks.insert(&key, serde_json::to_vec(&stored)?)?;
                // Re-push to the FIFO queue so the worker processes it.
                let entry = QueueEntry {
                    task_id: stored.task_id,
                    track_uri: stored.track_uri,
                    collection: Arc::new(stored.collection),
                };
                <Self as QueueStorage>::push(self, entry)?;
                recovered += 1;
            }
        }
        if recovered > 0 {
            info!("Recovered {recovered} interrupted task(s) from previous session");
        }
        Ok(())
    }
}

impl QueueStorage for SledStorage {
    fn push(&self, entry: QueueEntry) -> anyhow::Result<()> {
        let id = self.db.generate_id()?;
        let key = id.to_be_bytes();
        let stored = StoredEntry {
            task_id: entry.task_id,
            track_uri: entry.track_uri,
            collection: (*entry.collection).clone(),
        };
        let value = serde_json::to_vec(&stored)?;
        self.db.insert(key, value)?;
        Ok(())
    }

    fn pop(&self) -> anyhow::Result<Option<QueueEntry>> {
        // sled keeps keys in sorted order; the first key is always the oldest entry.
        let first = self.db.iter().next().transpose()?;
        let Some((key, value)) = first else {
            return Ok(None);
        };
        let stored: StoredEntry = match serde_json::from_slice(&value) {
            Ok(e) => e,
            Err(e) => {
                // Corrupt entry — remove it so it doesn't block the queue forever.
                warn!("Dropping corrupt queue entry (key {:?}): {e}", &*key);
                self.db.remove(key)?;
                return Ok(None);
            }
        };
        self.db.remove(key)?;
        Ok(Some(QueueEntry {
            task_id: stored.task_id,
            track_uri: stored.track_uri,
            collection: std::sync::Arc::new(stored.collection),
        }))
    }

    fn len(&self) -> usize {
        self.db.len()
    }
}

// ---------------------------------------------------------------------------
// TaskRegistry impl
// ---------------------------------------------------------------------------

impl TaskRegistry for SledStorage {
    fn register(&self, entry: &QueueEntry, status: TaskStatus) -> anyhow::Result<()> {
        // Enforce one-entry-per-track-uri: remove any previous record for this
        // URI before inserting the new one (handles retries and re-queues).
        let stale_keys: Vec<_> = self
            .tasks
            .iter()
            .filter_map(|res| {
                let (key, value) = res.ok()?;
                let stored: StoredTask = serde_json::from_slice(&value).ok()?;
                if stored.track_uri == entry.track_uri { Some(key) } else { None }
            })
            .collect();
        for key in stale_keys {
            self.tasks.remove(key)?;
        }

        let stored = StoredTask {
            task_id: entry.task_id,
            track_uri: entry.track_uri.clone(),
            collection: (*entry.collection).clone(),
            status,
        };
        self.tasks.insert(entry.task_id.as_bytes(), serde_json::to_vec(&stored)?)?;
        Ok(())
    }

    fn update(&self, task_id: TaskId, status: TaskStatus) -> anyhow::Result<()> {
        let key = task_id.as_bytes();
        let Some(bytes) = self.tasks.get(key)? else {
            warn!("TaskRegistry::update called for unknown task {task_id}");
            return Ok(());
        };
        let mut stored: StoredTask = serde_json::from_slice(&bytes)?;
        stored.status = status;
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
                })
            })
            .collect()
    }
}
