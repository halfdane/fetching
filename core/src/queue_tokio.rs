//! Tokio-based queue implementation.
//!
//! Provides:
//! - [`InMemoryStorage`] – the default `QueueStorage` backend (swap this for sled/yaque)
//! - [`TokioQueue`] – wires storage, async notification, progress broadcast, and the worker loop
//!
//! The worker loop enforces **single active download** via a `Semaphore(1)`.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{broadcast, Mutex, Notify, Semaphore};
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
        // QueueStorage::push is sync, but our Mutex is async.
        // Calling blocking_lock() is safe so long as we are never inside an
        // async context that would dead-lock on the same lock. Since push is
        // only called from TokioQueue::add_collection (sync context), this
        // is fine. Alternatively, callers may use try_lock and retry.
        self.entries.blocking_lock().push_back(entry);
        Ok(())
    }

    fn pop(&self) -> anyhow::Result<Option<QueueEntry>> {
        Ok(self.entries.blocking_lock().pop_front())
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
                    message: None,
                },
            );

            info!("Processing track: {} (task {})", entry.track_uri, task_id);

            // Run the blocking job off the async executor.
            let runner = Arc::clone(&runner);
            let apis = Arc::clone(&apis);
            let track_uri = entry.track_uri.clone();

            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit; // drop permit when job finishes
                runner.run(&entry, &apis)
            })
            .await;

            let update = match result {
                Ok(Ok(())) => {
                    info!("Task {task_id} done: {track_uri}");
                    ProgressUpdate {
                        task_id,
                        status: TaskStatus::Done,
                        message: None,
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
