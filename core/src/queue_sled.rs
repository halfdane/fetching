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

use sled::Db;
use tracing::warn;

use crate::container::TrackCollection;
use crate::queue::{QueueEntry, QueueStorage, TaskId};

/// Owned, serialisable mirror of `QueueEntry`.
/// `QueueEntry` uses `Arc<TrackCollection>` for zero-copy sharing in memory;
/// on disk we store plain owned values and reconstruct the `Arc` on load.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredEntry {
    task_id: TaskId,
    track_uri: String,
    collection: TrackCollection,
}

/// Sled-backed FIFO queue storage.
///
/// Each entry is serialised as JSON and stored at an auto-incrementing
/// big-endian `u64` key, which sled keeps in sorted order — so the
/// smallest key is always the oldest (FIFO) entry.
pub struct SledStorage {
    db: Db,
}

impl SledStorage {
    /// Open (or create) a sled database at `path`.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
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
