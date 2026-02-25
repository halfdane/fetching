//! Queue types, traits, and the replaceable storage seam.
//!
//! # Layers
//!
//! ```text
//! queue.rs        – data types + QueueStorage / JobRunner traits  (you are here)
//! queue_tokio.rs  – InMemoryStorage + TokioQueue worker loop
//! queue_sled.rs   – (future) SledStorage implementing QueueStorage
//! queue_yaque.rs  – (future) YaqueStorage implementing QueueStorage
//! ```
//!
//! Only `QueueStorage` needs to be replaced to swap the persistence backend.
//! The Tokio worker loop, semaphore, and progress broadcast stay in `queue_tokio.rs`.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::audio::AudioFileDownloader;
use crate::container::TrackCollection;
use crate::spotify_api::{SpotifyCollectionMetadata, SpotifyTrackMetadata};

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

pub type TaskId = Uuid;

// ---------------------------------------------------------------------------
// Task status & progress
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    /// `reason` is a human-readable error string.
    Failed { reason: String },
}

/// Track metadata resolved during download — sent in the `Running` update so
/// the frontend can replace placeholder titles with real information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackInfo {
    pub title: String,
    pub artists: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<i32>,
    pub duration_ms: i32,
}

/// Minimal progress payload sent over the SSE broadcast channel.
///
/// Intentionally kept narrow – add fields here when the frontend needs them,
/// without touching the storage or runner seams.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub task_id: TaskId,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Populated on the first `Running` update, once track metadata is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_info: Option<TrackInfo>,
}

// ---------------------------------------------------------------------------
// Queue entry
// ---------------------------------------------------------------------------

/// One entry per **track URI**. The parent collection is shared via `Arc`
/// so no data is duplicated across entries that belong to the same album/playlist.
#[derive(Clone, Debug)]
pub struct QueueEntry {
    pub task_id: TaskId,
    pub track_uri: String,
    pub collection: Arc<TrackCollection>,
}

impl QueueEntry {
    pub fn new(track_uri: impl Into<String>, collection: Arc<TrackCollection>) -> Self {
        Self {
            task_id: Uuid::new_v4(),
            track_uri: track_uri.into(),
            collection,
        }
    }
}

// ---------------------------------------------------------------------------
// CoverFetcher – object-safe wrapper around SpotifyCover
// ---------------------------------------------------------------------------

/// Object-safe cover-fetching trait used inside the queue.
///
/// `SpotifyCover` carries a `Clone` supertrait which makes it non-object-safe.
/// This thin trait drops `Clone` so that `Arc<dyn CoverFetcher>` compiles.
/// A blanket impl is provided for every `T: SpotifyCover`.
#[async_trait]
pub trait CoverFetcher: Send + Sync + 'static {
    async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>>;
}

#[async_trait]
impl<T> CoverFetcher for T
where
    T: crate::spotify_api::SpotifyCover + 'static,
{
    async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>> {
        crate::spotify_api::SpotifyCover::fetch_cover(self, cover_id).await
    }
}

// ---------------------------------------------------------------------------
// WorkerApis
// ---------------------------------------------------------------------------

/// All Spotify API handles a `JobRunner` needs, bundled for cheap `Arc` cloning.
pub struct WorkerApis {
    pub collection_metadata: Arc<dyn SpotifyCollectionMetadata + Send + Sync>,
    pub track_metadata: Arc<dyn SpotifyTrackMetadata + Send + Sync>,
    pub cover: Arc<dyn CoverFetcher>,
    pub audio: Arc<dyn AudioFileDownloader>,
}

// ---------------------------------------------------------------------------
// JobRunner – the work-phase entry point
// ---------------------------------------------------------------------------

/// The actual download / tag / store pipeline plugs in here.
///
/// `run` is **synchronous** on purpose: librespot's audio I/O blocks, so the queue
/// calls this inside `tokio::task::spawn_blocking` to keep the async executor free.
///
/// `progress_tx` is provided so the runner can emit mid-job updates (e.g. a
/// `Running` event carrying resolved [`TrackInfo`] or a stage description).
///
/// The returned `Option<String>` is a short human-readable message attached to the
/// final `Done` SSE event — e.g. `"Downloaded"` or `"File already exists"`.
pub trait JobRunner: Send + Sync + 'static {
    fn run(
        &self,
        entry: &QueueEntry,
        apis: &WorkerApis,
        progress_tx: &broadcast::Sender<ProgressUpdate>,
    ) -> anyhow::Result<Option<String>>;
}

// ---------------------------------------------------------------------------
// QueueStorage – THE REPLACEABLE SEAM
// ---------------------------------------------------------------------------

/// Push / pop interface for queue entries.
///
/// This is the only trait that needs to be implemented when swapping the
/// persistence backend (in-memory → sled → yaque etc.).
///
/// # Implementing a new backend
///
/// 1. Create `core/src/queue_sled.rs` (or `queue_yaque.rs`).
/// 2. Define `pub struct SledStorage { … }` and `impl QueueStorage for SledStorage`.
/// 3. Pass `Arc::new(SledStorage::open(path)?)` to `TokioQueue::with_storage(…)`.
/// 4. Nothing else changes – the worker loop, semaphore, and broadcast channel
///    live entirely in `queue_tokio.rs` and are backend-agnostic.
///
/// ## yaque note
/// yaque's push/pop are async. Wrap them with `Handle::current().block_on(…)` or
/// use `tokio::task::block_in_place` inside the `push`/`pop` impls so they satisfy
/// the sync `QueueStorage` contract while still driving the yaque futures.
pub trait QueueStorage: Send + Sync + 'static {
    /// Append an entry to the back of the queue.
    fn push(&self, entry: QueueEntry) -> anyhow::Result<()>;

    /// Remove and return the entry at the front of the queue, or `None` if empty.
    fn pop(&self) -> anyhow::Result<Option<QueueEntry>>;

    /// Return the number of entries currently waiting in the queue.
    fn len(&self) -> usize;

    /// Returns `true` if the queue is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{CollectionType, TrackCollection};

    fn fake_collection() -> Arc<TrackCollection> {
        Arc::new(TrackCollection {
            uri_str: "spotify:album:test".to_string(),
            collection_type: CollectionType::Album,
            title: "Test".to_string(),
            artists: vec![],
            cover_id: None,
            upc: None,
            total_tracks: 1,
            label: None,
            date: None,
            track_uris: vec!["spotify:track:aaa".to_string()],
        })
    }

    #[test]
    fn entry_new_assigns_unique_task_ids() {
        let col = fake_collection();
        let a = QueueEntry::new("spotify:track:aaa", Arc::clone(&col));
        let b = QueueEntry::new("spotify:track:aaa", Arc::clone(&col));
        assert_ne!(a.task_id, b.task_id);
    }

    #[test]
    fn entry_new_stores_track_uri() {
        let col = fake_collection();
        let entry = QueueEntry::new("spotify:track:bbb", col);
        assert_eq!(entry.track_uri, "spotify:track:bbb");
    }

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

    #[test]
    fn progress_update_serde_round_trips_with_message() {
        let update = ProgressUpdate {
            task_id: Uuid::new_v4(),
            status: TaskStatus::Running,
            message: Some("fetching".to_string()),
            track_info: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        let back: ProgressUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_id, update.task_id);
        assert_eq!(back.status, update.status);
        assert_eq!(back.message, update.message);
    }

    #[test]
    fn progress_update_omits_message_field_when_none() {
        let update = ProgressUpdate {
            task_id: Uuid::new_v4(),
            status: TaskStatus::Done,
            message: None,
            track_info: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(!json.contains("message"), "message field should be omitted: {json}");
    }
}
