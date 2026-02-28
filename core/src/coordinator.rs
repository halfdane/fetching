//! Download coordinator.
//!
//! [`DownloadCoordinator`] is the central orchestration point:
//! - accepts collections and fans them into per-track tasks
//! - feeds them one-at-a-time to a blocking worker via a background loop
//! - persists every status transition to the SQLite [`Database`]
//! - broadcasts progress over an SSE-friendly channel

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Notify, Semaphore};
use tracing::{error, info, warn};

use crate::audio::AudioFileDownloader;
use crate::container::TrackCollection;
use crate::db::{Database, TaskStatus};
use crate::queue::{QueueEntry, TaskId};
use crate::queue_memory::TrackQueue;
use crate::spotify_api::{CoverFetcher, SpotifyCollectionMetadata, SpotifyTrackMetadata};

// ---------------------------------------------------------------------------
// Progress types (SSE broadcast payload)
// ---------------------------------------------------------------------------

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
/// without touching the database or runner seams.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub task_id: TaskId,
    /// Which collection this task belongs to — lets the frontend route events
    /// to the correct collection tile without a client-side lookup map.
    pub collection_id: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Populated on the first `Running` update, once track metadata is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_info: Option<TrackInfo>,
}

// ---------------------------------------------------------------------------
// WorkerApis
// ---------------------------------------------------------------------------

/// All Spotify API handles a [`JobRunner`] needs, bundled for cheap `Arc` cloning.
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
/// `run` is **synchronous** on purpose: librespot's audio I/O blocks, so the
/// coordinator calls this inside `tokio::task::spawn_blocking` to keep the
/// async executor free.
///
/// `on_progress` is a callback the runner invokes at every state transition
/// (resolved metadata, stage changes, retries).  The coordinator wires it to
/// both the SSE broadcast channel **and** the persistent database so that
/// mid-job updates survive a server restart.
///
/// The returned `Option<String>` is a short human-readable message attached to
/// the final `Done` SSE event — e.g. `"Downloaded"` or `"File already exists"`.
pub trait JobRunner: Send + Sync + 'static {
    fn run(
        &self,
        entry: &QueueEntry,
        apis: &WorkerApis,
        collection_id: &str,
        on_progress: &dyn Fn(ProgressUpdate),
    ) -> anyhow::Result<Option<String>>;
}

// ---------------------------------------------------------------------------
// DownloadCoordinator
// ---------------------------------------------------------------------------

/// User-facing handle for the download pipeline.
///
/// A single `DownloadCoordinator` instance is shared (as `Arc<DownloadCoordinator>`)
/// between the HTTP layer and the background worker task.
///
/// # Example
///
/// ```ignore
/// let coord = Arc::new(DownloadCoordinator::new(apis, runner));
/// coord.start();
///
/// // In the HTTP handler:
/// coord.add_collection(Arc::new(collection));
///
/// // In the SSE handler:
/// let rx = coord.subscribe_progress();
/// ```
pub struct DownloadCoordinator {
    queue: TrackQueue,
    db: Option<Arc<Database>>,
    notify: Arc<Notify>,
    progress_tx: broadcast::Sender<ProgressUpdate>,
    apis: Arc<WorkerApis>,
    runner: Arc<dyn JobRunner>,
}

impl DownloadCoordinator {
    const PROGRESS_CHANNEL_CAPACITY: usize = 256;

    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// Create a coordinator with no database (batch mode).
    pub fn new(apis: WorkerApis, runner: impl JobRunner) -> Self {
        let (progress_tx, _) = broadcast::channel(Self::PROGRESS_CHANNEL_CAPACITY);
        Self {
            queue: TrackQueue::new(),
            db: None,
            notify: Arc::new(Notify::new()),
            progress_tx,
            apis: Arc::new(apis),
            runner: Arc::new(runner),
        }
    }

    /// Create a coordinator backed by a persistent SQLite database (server mode).
    ///
    /// The database records every status transition so that `GET /api/collections`
    /// can return a full snapshot even after page refresh.
    pub fn with_db(
        db: Arc<Database>,
        apis: WorkerApis,
        runner: impl JobRunner,
    ) -> Self {
        let (progress_tx, _) = broadcast::channel(Self::PROGRESS_CHANNEL_CAPACITY);
        Self {
            queue: TrackQueue::new(),
            db: Some(db),
            notify: Arc::new(Notify::new()),
            progress_tx,
            apis: Arc::new(apis),
            runner: Arc::new(runner),
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Enqueue one `QueueEntry` per `track_uri` in the collection.
    ///
    /// Returns `(collection_id, Vec<(track_id, task_id)>)` when backed by a
    /// database, or a generated collection_id with task_ids when in batch mode.
    pub fn add_collection(&self, collection: Arc<TrackCollection>) -> (String, Vec<(String, String)>) {
        if let Some(db) = &self.db {
            match db.insert_collection_with_tracks(&collection) {
                Ok((collection_id, id_pairs)) => {
                    for (_, task_id) in &id_pairs {
                        let task_uuid = uuid::Uuid::parse_str(task_id)
                            .unwrap_or_else(|_| uuid::Uuid::new_v4());
                        // Find the track_uri for this task
                        if let Ok(Some((_, track_uri))) = db.collection_for_task(task_id) {
                            let entry = QueueEntry {
                                task_id: task_uuid,
                                track_uri,
                                collection: Arc::clone(&collection),
                            };
                            self.queue.push(entry);
                        }
                    }
                    self.notify.notify_one();
                    (collection_id, id_pairs)
                }
                Err(e) => {
                    error!("Failed to insert collection into database: {e}");
                    // Fall through to batch-mode style
                    let collection_id = uuid::Uuid::new_v4().to_string();
                    let pairs = self.enqueue_batch(&collection, &collection_id);
                    (collection_id, pairs)
                }
            }
        } else {
            let collection_id = uuid::Uuid::new_v4().to_string();
            let pairs = self.enqueue_batch(&collection, &collection_id);
            (collection_id, pairs)
        }
    }

    /// Batch-mode enqueue (no database).
    fn enqueue_batch(
        &self,
        collection: &Arc<TrackCollection>,
        _collection_id: &str,
    ) -> Vec<(String, String)> {
        let pairs: Vec<(String, String)> = collection
            .track_uris
            .iter()
            .map(|uri| {
                let entry = QueueEntry::new(uri.clone(), Arc::clone(collection));
                let task_id = entry.task_id.to_string();
                let track_id = uuid::Uuid::new_v4().to_string();
                self.queue.push(entry);
                (track_id, task_id)
            })
            .collect();
        self.notify.notify_one();
        pairs
    }

    /// Push a single pre-built entry (used for crash-recovery re-queue).
    pub fn enqueue(&self, entry: QueueEntry) {
        self.queue.push(entry);
    }

    /// Subscribe to the progress broadcast channel.
    ///
    /// The server's SSE handler should call this once per connection and stream
    /// the received `ProgressUpdate` values as JSON-encoded SSE events.
    pub fn subscribe_progress(&self) -> broadcast::Receiver<ProgressUpdate> {
        self.progress_tx.subscribe()
    }

    /// Return the number of entries currently waiting in the queue.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns `true` if the queue has no pending entries.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Get a reference to the database, if configured.
    pub fn db(&self) -> Option<&Arc<Database>> {
        self.db.as_ref()
    }

    /// Spawn the background worker loop.
    ///
    /// Call this exactly once during application startup.
    /// The loop runs until the Tokio runtime shuts down.
    /// If there are already pending entries (e.g. recovered from DB after a
    /// restart), the worker is woken immediately.
    pub fn start(self: &Arc<Self>) {
        // Wake the worker for any entries already queued at startup.
        if !self.queue.is_empty() {
            self.notify.notify_one();
        }
        tokio::spawn(worker_loop(Arc::clone(self)));
    }
}

// ---------------------------------------------------------------------------
// Worker loop (private)
// ---------------------------------------------------------------------------

/// Main worker task. Processes queue entries one at a time.
///
/// The `Semaphore(1)` makes the single-download constraint explicit and
/// easy to relax later (increase the permits to allow parallel downloads).
async fn worker_loop(coord: Arc<DownloadCoordinator>) {
    let semaphore = Arc::new(Semaphore::new(1));

    loop {
        // Block until add_collection / enqueue wakes us.
        coord.notify.notified().await;

        // Drain all pending entries, one at a time.
        loop {
            let Some(entry) = coord.queue.pop() else {
                break; // queue empty, go back to waiting
            };

            let task_id = entry.task_id;

            // Look up which collection this task belongs to (for SSE routing).
            let collection_id = coord
                .db
                .as_ref()
                .and_then(|db| {
                    db.collection_for_task(&task_id.to_string())
                        .ok()
                        .flatten()
                        .map(|(cid, _)| cid)
                })
                .unwrap_or_default();

            // Acquire the "single active download" slot.
            let permit = match semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    warn!("Semaphore closed; worker loop exiting");
                    return;
                }
            };

            emit_update(
                &coord.progress_tx,
                coord.db.as_ref(),
                ProgressUpdate {
                    task_id,
                    collection_id: collection_id.clone(),
                    status: TaskStatus::Running,
                    message: Some("Running…".to_string()),
                    track_info: None,
                },
            );

            info!("Processing track: {} (task {})", entry.track_uri, task_id);

            // Run the blocking job off the async executor.
            let runner = Arc::clone(&coord.runner);
            let apis = Arc::clone(&coord.apis);
            let track_uri = entry.track_uri.clone();
            let progress_tx_for_runner = coord.progress_tx.clone();
            let db_for_runner = coord.db.clone();
            let collection_id_for_runner = collection_id.clone();

            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit; // drop permit when job finishes
                let on_progress = |update: ProgressUpdate| {
                    emit_update(
                        &progress_tx_for_runner,
                        db_for_runner.as_ref(),
                        update,
                    );
                };
                runner.run(&entry, &apis, &collection_id_for_runner, &on_progress)
            })
            .await;

            let update = match result {
                Ok(Ok(final_msg)) => {
                    info!("Task {task_id} done: {track_uri}");
                    ProgressUpdate {
                        task_id,
                        collection_id: collection_id.clone(),
                        status: TaskStatus::Done,
                        message: final_msg,
                        track_info: None,
                    }
                }
                Ok(Err(e)) => {
                    error!("Task {task_id} failed: {e}");
                    ProgressUpdate {
                        task_id,
                        collection_id: collection_id.clone(),
                        status: TaskStatus::Failed {
                            reason: e.to_string(),
                        },
                        message: None,
                        track_info: None,
                    }
                }
                Err(e) => {
                    error!("Task {task_id} panicked: {e}");
                    ProgressUpdate {
                        task_id,
                        collection_id: collection_id.clone(),
                        status: TaskStatus::Failed {
                            reason: format!("worker panic: {e}"),
                        },
                        message: None,
                        track_info: None,
                    }
                }
            };

            emit_update(&coord.progress_tx, coord.db.as_ref(), update);
        }
    }
}

fn emit_update(
    tx: &broadcast::Sender<ProgressUpdate>,
    db: Option<&Arc<Database>>,
    update: ProgressUpdate,
) {
    // Always persist to the database, regardless of whether anyone is
    // subscribed to the SSE stream.
    if let Some(db) = db {
        if let Err(e) = db.update_task(
            &update.task_id.to_string(),
            &update.status,
            update.message.as_deref(),
        ) {
            warn!("Failed to update DB for task {}: {e}", update.task_id);
        }
    }
    if tx.receiver_count() > 0 {
        if let Err(e) = tx.send(update) {
            warn!("Failed to send progress update: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{CollectionType, TrackCollection};
    use crate::spotify_api::CoverFetcher;
    use async_trait::async_trait;
    use librespot_core::SpotifyUri;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::time::timeout;

    // -----------------------------------------------------------------------
    // Stub implementations (none of the workers below actually call these)
    // -----------------------------------------------------------------------

    struct StubCollectionMetadata;
    impl crate::spotify_api::SpotifyCollectionMetadata for StubCollectionMetadata {
        fn fetch_album(&self, _: &SpotifyUri) -> anyhow::Result<TrackCollection> { unimplemented!() }
        fn fetch_playlist(&self, _: &SpotifyUri) -> anyhow::Result<TrackCollection> { unimplemented!() }
        fn fetch_track(&self, _: &SpotifyUri) -> anyhow::Result<TrackCollection> { unimplemented!() }
        fn fetch_episode(&self, _: &SpotifyUri) -> anyhow::Result<TrackCollection> { unimplemented!() }
        fn fetch_show(&self, _: &SpotifyUri) -> anyhow::Result<TrackCollection> { unimplemented!() }
    }

    struct StubTrackMetadata;
    impl crate::spotify_api::SpotifyTrackMetadata for StubTrackMetadata {
        fn fetch_single_episode(&self, _: &SpotifyUri) -> anyhow::Result<crate::container::Track> { unimplemented!() }
        fn fetch_single_track(&self, _: &SpotifyUri) -> anyhow::Result<crate::container::Track> { unimplemented!() }
    }

    struct StubCoverFetcher;
    #[async_trait]
    impl CoverFetcher for StubCoverFetcher {
        async fn fetch_cover(&self, _: &str) -> anyhow::Result<Vec<u8>> { unimplemented!() }
    }

    struct StubAudioDownloader;
    impl crate::audio::AudioFileDownloader for StubAudioDownloader {
        fn download(&self, _: &str, _: &std::path::Path, _: &dyn Fn(u32, u32, u64)) -> anyhow::Result<crate::audio::DownloadedTrack> { unimplemented!() }
    }

    fn stub_apis() -> WorkerApis {
        WorkerApis {
            collection_metadata: Arc::new(StubCollectionMetadata),
            track_metadata: Arc::new(StubTrackMetadata),
            cover: Arc::new(StubCoverFetcher),
            audio: Arc::new(StubAudioDownloader),
        }
    }

    fn fake_collection(uris: &[&str]) -> Arc<TrackCollection> {
        Arc::new(TrackCollection {
            uri_str: "spotify:album:test".to_string(),
            collection_type: CollectionType::Album,
            title: "Test".to_string(),
            artists: vec![],
            cover_id: None,
            upc: None,
            total_tracks: uris.len(),
            label: None,
            date: None,
            track_uris: uris.iter().map(|s| s.to_string()).collect(),
        })
    }

    struct OkRunner;
    impl JobRunner for OkRunner {
        fn run(&self, _: &QueueEntry, _: &WorkerApis, _: &str, _: &dyn Fn(ProgressUpdate)) -> anyhow::Result<Option<String>> { Ok(None) }
    }

    struct FailRunner;
    impl JobRunner for FailRunner {
        fn run(&self, _: &QueueEntry, _: &WorkerApis, _: &str, _: &dyn Fn(ProgressUpdate)) -> anyhow::Result<Option<String>> {
            anyhow::bail!("intentional failure")
        }
    }

    // -----------------------------------------------------------------------
    // DownloadCoordinator::add_collection
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn add_collection_returns_one_task_id_per_track_uri() {
        let coord = DownloadCoordinator::new(stub_apis(), OkRunner);
        let (_, pairs) = coord.add_collection(fake_collection(&["uri:1", "uri:2", "uri:3"]));
        assert_eq!(pairs.len(), 3);
        let unique: HashSet<_> = pairs.iter().map(|(_, tid)| tid).collect();
        assert_eq!(unique.len(), 3, "task IDs must be unique");
    }

    #[tokio::test]
    async fn add_collection_pushes_entries_in_order() {
        let coord = DownloadCoordinator::new(stub_apis(), OkRunner);
        coord.add_collection(fake_collection(&["uri:x", "uri:y"]));
        assert_eq!(coord.queue.pop().unwrap().track_uri, "uri:x");
        assert_eq!(coord.queue.pop().unwrap().track_uri, "uri:y");
        assert!(coord.queue.pop().is_none());
    }

    // -----------------------------------------------------------------------
    // Worker loop – progress events
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn worker_sends_running_then_done_on_success() {
        let coord = Arc::new(DownloadCoordinator::new(stub_apis(), OkRunner));
        let mut rx = coord.subscribe_progress();
        coord.start();

        let (_, pairs) = coord.add_collection(fake_collection(&["uri:1"]));
        let task_ids: HashSet<TaskId> = pairs.iter()
            .filter_map(|(_, tid)| uuid::Uuid::parse_str(tid).ok())
            .collect();

        let running = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Running").unwrap();
        let done = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Done").unwrap();

        assert!(task_ids.contains(&running.task_id));
        assert_eq!(running.status, TaskStatus::Running);
        assert!(task_ids.contains(&done.task_id));
        assert_eq!(done.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn worker_sends_running_then_failed_on_error() {
        let coord = Arc::new(DownloadCoordinator::new(stub_apis(), FailRunner));
        let mut rx = coord.subscribe_progress();
        coord.start();

        let (_, pairs) = coord.add_collection(fake_collection(&["uri:1"]));
        let task_ids: HashSet<TaskId> = pairs.iter()
            .filter_map(|(_, tid)| uuid::Uuid::parse_str(tid).ok())
            .collect();

        let running = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Running").unwrap();
        let failed = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Failed").unwrap();

        assert!(task_ids.contains(&running.task_id));
        assert_eq!(running.status, TaskStatus::Running);
        assert!(task_ids.contains(&failed.task_id));
        assert!(
            matches!(failed.status, TaskStatus::Failed { ref reason } if reason.contains("intentional")),
            "unexpected status: {:?}", failed.status
        );
    }

    #[tokio::test]
    async fn worker_sends_progress_for_every_track_in_collection() {
        let coord = Arc::new(DownloadCoordinator::new(stub_apis(), OkRunner));
        let mut rx = coord.subscribe_progress();
        coord.start();

        let (_, pairs) = coord.add_collection(fake_collection(&["uri:1", "uri:2", "uri:3"]));
        let task_ids: HashSet<TaskId> = pairs.iter()
            .filter_map(|(_, tid)| uuid::Uuid::parse_str(tid).ok())
            .collect();

        let mut done_ids = HashSet::new();
        while done_ids.len() < 3 {
            let update = timeout(Duration::from_secs(5), rx.recv()).await
                .expect("timed out").unwrap();
            if task_ids.contains(&update.task_id) && update.status == TaskStatus::Done {
                done_ids.insert(update.task_id);
            }
        }
        assert_eq!(done_ids, task_ids);
    }

    // -----------------------------------------------------------------------
    // Single-concurrency contract
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn worker_runs_at_most_one_job_at_a_time() {
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        struct CountingRunner {
            concurrent: Arc<AtomicUsize>,
            max_seen: Arc<AtomicUsize>,
        }
        impl JobRunner for CountingRunner {
            fn run(&self, _: &QueueEntry, _: &WorkerApis, _: &str, _: &dyn Fn(ProgressUpdate)) -> anyhow::Result<Option<String>> {
                let c = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_seen.fetch_max(c, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                self.concurrent.fetch_sub(1, Ordering::SeqCst);
                Ok(None)
            }
        }

        let runner = CountingRunner {
            concurrent,
            max_seen: Arc::clone(&max_seen),
        };
        let coord = Arc::new(DownloadCoordinator::new(stub_apis(), runner));
        let mut rx = coord.subscribe_progress();
        coord.start();

        let (_, pairs) = coord.add_collection(fake_collection(&["uri:1", "uri:2", "uri:3"]));
        let task_ids: HashSet<TaskId> = pairs.iter()
            .filter_map(|(_, tid)| uuid::Uuid::parse_str(tid).ok())
            .collect();

        let mut done_count = 0;
        while done_count < 3 {
            let update = timeout(Duration::from_secs(5), rx.recv()).await
                .expect("timed out").unwrap();
            if task_ids.contains(&update.task_id) && update.status == TaskStatus::Done {
                done_count += 1;
            }
        }

        assert_eq!(
            max_seen.load(Ordering::SeqCst), 1,
            "more than one job ran concurrently"
        );
    }

    // -- ProgressUpdate serde contract (wire format shared with the frontend) -

    #[test]
    fn progress_update_serde_round_trips_with_message() {
        let update = ProgressUpdate {
            task_id: TaskId::new_v4(),
            collection_id: "test-coll".to_string(),
            status: TaskStatus::Running,
            message: Some("fetching".to_string()),
            track_info: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        let back: ProgressUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_id, update.task_id);
        assert_eq!(back.collection_id, update.collection_id);
        assert_eq!(back.status, update.status);
        assert_eq!(back.message, update.message);
    }

    #[test]
    fn progress_update_omits_message_field_when_none() {
        let update = ProgressUpdate {
            task_id: TaskId::new_v4(),
            collection_id: "test-coll".to_string(),
            status: TaskStatus::Done,
            message: None,
            track_info: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(!json.contains("message"), "message field should be omitted: {json}");
    }
}
