//! Tokio-based queue implementation.
//!
//! Provides:
//! - [`InMemoryStorage`] – the default `QueueStorage` backend (swap this for sled/yaque)
//! - [`TokioQueue`] – wires storage, async notification, progress broadcast, and the worker loop
//!
//! The worker loop enforces **single active download** via a `Semaphore(1)`.

use std::collections::VecDeque;
use std::sync::Arc;

use std::sync::Mutex;
use tokio::sync::{broadcast, Notify, Semaphore};
use tracing::{error, info, warn};

use crate::container::TrackCollection;
use crate::queue::{JobRunner, ProgressUpdate, QueueEntry, QueueStorage, TaskId, TaskStatus, WorkerApis};

// ---------------------------------------------------------------------------
// InMemoryStorage  (the default, replaceable backend)
// ---------------------------------------------------------------------------

/// FIFO in-memory queue storage. Replace with `SledStorage` or `YaqueStorage`
/// by implementing `QueueStorage` in a new file and passing it to
/// `TokioQueue::with_storage`.
pub struct InMemoryStorage {
    entries: Mutex<VecDeque<QueueEntry>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueStorage for InMemoryStorage {
    fn push(&self, entry: QueueEntry) -> anyhow::Result<()> {
        self.entries
            .lock()
            .expect("InMemoryStorage mutex poisoned")
            .push_back(entry);
        Ok(())
    }

    fn pop(&self) -> anyhow::Result<Option<QueueEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("InMemoryStorage mutex poisoned")
            .pop_front())
    }

    fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("InMemoryStorage mutex poisoned")
            .len()
    }
}

// ---------------------------------------------------------------------------
// TokioQueue
// ---------------------------------------------------------------------------

/// User-facing queue handle.
///
/// A single `TokioQueue` instance is shared (as `Arc<TokioQueue>`) between the
/// server state and the worker background task.
///
/// # Example
///
/// ```ignore
/// let queue = Arc::new(TokioQueue::new(apis, runner));
/// queue.start();
///
/// // In the HTTP handler:
/// queue.add_collection(Arc::new(collection));
///
/// // In the SSE handler:
/// let rx = queue.subscribe_progress();
/// ```
pub struct TokioQueue {
    storage: Arc<dyn QueueStorage>,
    notify: Arc<Notify>,
    progress_tx: broadcast::Sender<ProgressUpdate>,
    apis: Arc<WorkerApis>,
    runner: Arc<dyn JobRunner>,
}

impl TokioQueue {
    const PROGRESS_CHANNEL_CAPACITY: usize = 256;

    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// Create a queue backed by `InMemoryStorage`.
    pub fn new(apis: WorkerApis, runner: impl JobRunner) -> Self {
        Self::with_storage(InMemoryStorage::new(), apis, runner)
    }

    /// Create a queue backed by a custom `QueueStorage` implementation.
    ///
    /// Use this when swapping in a sled or yaque backend:
    /// ```ignore
    /// TokioQueue::with_storage(SledStorage::open(path)?, apis, runner)
    /// ```
    pub fn with_storage(storage: impl QueueStorage, apis: WorkerApis, runner: impl JobRunner) -> Self {
        let (progress_tx, _) = broadcast::channel(Self::PROGRESS_CHANNEL_CAPACITY);
        Self {
            storage: Arc::new(storage),
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
    /// Returns the `TaskId`s assigned to each entry so callers can correlate
    /// future `ProgressUpdate` messages.
    pub fn add_collection(&self, collection: Arc<TrackCollection>) -> Vec<TaskId> {
        let task_ids: Vec<TaskId> = collection
            .track_uris
            .iter()
            .map(|uri| {
                let entry = QueueEntry::new(uri.clone(), Arc::clone(&collection));
                let id = entry.task_id;
                if let Err(e) = self.storage.push(entry) {
                    error!("Failed to push queue entry for {uri}: {e}");
                }
                id
            })
            .collect();

        self.notify.notify_one();
        task_ids
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
        self.storage.len()
    }

    /// Returns `true` if the queue has no pending entries.
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Spawn the background worker loop.
    ///
    /// Call this exactly once during application startup.
    /// The loop runs until the Tokio runtime shuts down.
    pub fn start(&self) {
        let storage = Arc::clone(&self.storage);
        let notify = Arc::clone(&self.notify);
        let progress_tx = self.progress_tx.clone();
        let apis = Arc::clone(&self.apis);
        let runner = Arc::clone(&self.runner);

        tokio::spawn(worker_loop(storage, notify, progress_tx, apis, runner));
    }
}

// ---------------------------------------------------------------------------
// Worker loop (private)
// ---------------------------------------------------------------------------

/// Main worker task. Processes queue entries one at a time.
///
/// The `Semaphore(1)` makes the single-download constraint explicit and
/// easy to relax later (increase the permits to allow parallel downloads).
async fn worker_loop(
    storage: Arc<dyn QueueStorage>,
    notify: Arc<Notify>,
    progress_tx: broadcast::Sender<ProgressUpdate>,
    apis: Arc<WorkerApis>,
    runner: Arc<dyn JobRunner>,
) {
    let semaphore = Arc::new(Semaphore::new(1));

    loop {
        // Block until add_collection wakes us.
        notify.notified().await;

        // Drain all pending entries, one at a time.
        loop {
            let entry = match storage.pop() {
                Ok(Some(e)) => e,
                Ok(None) => break, // queue empty, go back to waiting
                Err(e) => {
                    error!("QueueStorage::pop failed: {e}");
                    break;
                }
            };

            let task_id = entry.task_id;

            // Acquire the "single active download" slot.
            // Increasing Semaphore capacity here → concurrent downloads.
            let permit = match semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    warn!("Semaphore closed; worker loop exiting");
                    return;
                }
            };

            send_progress(
                &progress_tx,
                ProgressUpdate {
                    task_id,
                    status: TaskStatus::Running,
                    message: Some("Running…".to_string()),
                    track_info: None,
                },
            );

            info!("Processing track: {} (task {})", entry.track_uri, task_id);

            // Run the blocking job off the async executor.
            let runner = Arc::clone(&runner);
            let apis = Arc::clone(&apis);
            let track_uri = entry.track_uri.clone();
            let progress_tx_for_runner = progress_tx.clone();

            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit; // drop permit when job finishes
                runner.run(&entry, &apis, &progress_tx_for_runner)
            })
            .await;

            let update = match result {
                Ok(Ok(final_msg)) => {
                    info!("Task {task_id} done: {track_uri}");
                    ProgressUpdate {
                        task_id,
                        status: TaskStatus::Done,
                        message: final_msg,
                        track_info: None,
                    }
                }
                Ok(Err(e)) => {
                    error!("Task {task_id} failed: {e}");
                    ProgressUpdate {
                        task_id,
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
                        status: TaskStatus::Failed {
                            reason: format!("worker panic: {e}"),
                        },
                        message: None,
                        track_info: None,
                    }
                }
            };

            send_progress(&progress_tx, update);
        }
    }
}

fn send_progress(tx: &broadcast::Sender<ProgressUpdate>, update: ProgressUpdate) {
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
    use crate::queue::{CoverFetcher, JobRunner, ProgressUpdate, QueueEntry, TaskStatus, WorkerApis};
    use async_trait::async_trait;
    use librespot_core::SpotifyUri;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::broadcast;
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
        fn run(&self, _: &QueueEntry, _: &WorkerApis, _: &broadcast::Sender<ProgressUpdate>) -> anyhow::Result<Option<String>> { Ok(None) }
    }

    struct FailRunner;
    impl JobRunner for FailRunner {
        fn run(&self, _: &QueueEntry, _: &WorkerApis, _: &broadcast::Sender<ProgressUpdate>) -> anyhow::Result<Option<String>> {
            anyhow::bail!("intentional failure")
        }
    }

    // -----------------------------------------------------------------------
    // InMemoryStorage
    // -----------------------------------------------------------------------

    #[test]
    fn storage_pop_on_empty_returns_none() {
        let s = InMemoryStorage::new();
        assert!(s.pop().unwrap().is_none());
    }

    #[test]
    fn storage_preserves_fifo_order() {
        let s = InMemoryStorage::new();
        let col = fake_collection(&["uri:a", "uri:b", "uri:c"]);
        for uri in ["uri:a", "uri:b", "uri:c"] {
            s.push(QueueEntry::new(uri, Arc::clone(&col))).unwrap();
        }
        assert_eq!(s.pop().unwrap().unwrap().track_uri, "uri:a");
        assert_eq!(s.pop().unwrap().unwrap().track_uri, "uri:b");
        assert_eq!(s.pop().unwrap().unwrap().track_uri, "uri:c");
        assert!(s.pop().unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // TokioQueue::add_collection
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn add_collection_returns_one_task_id_per_track_uri() {
        let queue = TokioQueue::new(stub_apis(), OkRunner);
        let ids = queue.add_collection(fake_collection(&["uri:1", "uri:2", "uri:3"]));
        assert_eq!(ids.len(), 3);
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3, "task IDs must be unique");
    }

    #[tokio::test]
    async fn add_collection_pushes_entries_to_storage_in_order() {
        let queue = TokioQueue::new(stub_apis(), OkRunner);
        queue.add_collection(fake_collection(&["uri:x", "uri:y"]));
        // access storage directly – test submodule can see private fields
        assert_eq!(queue.storage.pop().unwrap().unwrap().track_uri, "uri:x");
        assert_eq!(queue.storage.pop().unwrap().unwrap().track_uri, "uri:y");
        assert!(queue.storage.pop().unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // Worker loop – progress events
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn worker_sends_running_then_done_on_success() {
        let queue = Arc::new(TokioQueue::new(stub_apis(), OkRunner));
        let mut rx = queue.subscribe_progress();
        queue.start();

        let ids: HashSet<_> = queue
            .add_collection(fake_collection(&["uri:1"]))
            .into_iter()
            .collect();

        let running = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Running").unwrap();
        let done = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Done").unwrap();

        assert!(ids.contains(&running.task_id));
        assert_eq!(running.status, TaskStatus::Running);
        assert!(ids.contains(&done.task_id));
        assert_eq!(done.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn worker_sends_running_then_failed_on_error() {
        let queue = Arc::new(TokioQueue::new(stub_apis(), FailRunner));
        let mut rx = queue.subscribe_progress();
        queue.start();

        let ids: HashSet<_> = queue
            .add_collection(fake_collection(&["uri:1"]))
            .into_iter()
            .collect();

        let running = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Running").unwrap();
        let failed = timeout(Duration::from_secs(2), rx.recv()).await
            .expect("timed out waiting for Failed").unwrap();

        assert!(ids.contains(&running.task_id));
        assert_eq!(running.status, TaskStatus::Running);
        assert!(ids.contains(&failed.task_id));
        assert!(
            matches!(failed.status, TaskStatus::Failed { ref reason } if reason.contains("intentional")),
            "unexpected status: {:?}", failed.status
        );
    }

    #[tokio::test]
    async fn worker_sends_progress_for_every_track_in_collection() {
        let queue = Arc::new(TokioQueue::new(stub_apis(), OkRunner));
        let mut rx = queue.subscribe_progress();
        queue.start();

        let ids: HashSet<_> = queue
            .add_collection(fake_collection(&["uri:1", "uri:2", "uri:3"]))
            .into_iter()
            .collect();

        let mut done_ids = HashSet::new();
        while done_ids.len() < 3 {
            let update = timeout(Duration::from_secs(5), rx.recv()).await
                .expect("timed out").unwrap();
            if ids.contains(&update.task_id) && update.status == TaskStatus::Done {
                done_ids.insert(update.task_id);
            }
        }
        assert_eq!(done_ids, ids);
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
            fn run(&self, _: &QueueEntry, _: &WorkerApis, _: &broadcast::Sender<ProgressUpdate>) -> anyhow::Result<Option<String>> {
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
        let queue = Arc::new(TokioQueue::new(stub_apis(), runner));
        let mut rx = queue.subscribe_progress();
        queue.start();

        let ids: HashSet<_> = queue
            .add_collection(fake_collection(&["uri:1", "uri:2", "uri:3"]))
            .into_iter()
            .collect();

        let mut done_count = 0;
        while done_count < 3 {
            let update = timeout(Duration::from_secs(5), rx.recv()).await
                .expect("timed out").unwrap();
            if ids.contains(&update.task_id) && update.status == TaskStatus::Done {
                done_count += 1;
            }
        }

        assert_eq!(
            max_seen.load(Ordering::SeqCst), 1,
            "more than one job ran concurrently"
        );
    }
}
