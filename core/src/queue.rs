use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;
use crate::progress::{ProgressUpdate, ProgressScope, init_progress_tx};

#[derive(Debug, Clone)]
pub struct Task {
    pub task_id: Uuid,
    pub uri: String,
}

#[derive(Clone)]
pub struct SharedQueue {
    pub tasks: Arc<RwLock<Vec<Task>>>,
    session: Arc<librespot_core::Session>,
    config: Arc<RwLock<crate::config::Config>>,
    progress_tx: tokio::sync::broadcast::Sender<ProgressUpdate>,
}

unsafe impl Send for SharedQueue {}
unsafe impl Sync for SharedQueue {}

impl SharedQueue {
    pub fn new(
        session: Arc<librespot_core::Session>,
        config: crate::config::Config,
        capacity: usize,
    ) -> (Arc<Self>, broadcast::Receiver<ProgressUpdate>) {
        let rx = init_progress_tx(capacity);
        let tx = crate::progress::PROGRESS_TX.get().unwrap().clone();

        let queue = Arc::new(Self {
            session,
            tasks: Arc::new(RwLock::new(Vec::new())),
            config: Arc::new(RwLock::new(config)),
            progress_tx: tx,
        });

        (queue, rx)
    }

    pub async fn add_tasks(&self, uris: Vec<String>) {
        tracing::info!("Adding {} tasks", uris.len());
        let mut tasks = self.tasks.write().await;
        for uri in uris {
            let task = Task {
                task_id: Uuid::new_v4(),
                uri: uri.clone(),
            };
            let reporter = crate::progress::ProgressReporter {
                task_id: task.task_id,
                tx: self.progress_tx.clone(),
            };
            reporter.send(ProgressUpdate {
                task_id: task.task_id, // will be overwritten by reporter.send
                scope: ProgressScope::Playlist,
                status: "Adding to queue".to_string(),
                current: 0,
                total: 1,
                user_visible_identifier: Some(task.uri.clone())
            });
            tasks.push(task.clone());
            tracing::debug!("Pushed task: {:?}", task.task_id);
        }
        tracing::info!("Queue now has {} tasks", tasks.len());
    }

    pub async fn process_next(&self) -> Option<()> {
        let mut tasks = self.tasks.write().await;
        let Some(task) = tasks.pop() else {
            return None;
        };
        drop(tasks);
        tracing::info!(">>> START PROCESSING {}: {}", task.task_id, task.uri);

        let session = Arc::clone(&self.session);
        let config_guard = self.config.read().await;
        let config = (*config_guard).clone();
        let task_id = task.task_id;
        let uri = task.uri;
        let reporter = crate::progress::ProgressReporter {
            task_id,
            tx: self.progress_tx.clone(),
        };

        let handle = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            rt.block_on(crate::processor::process_url(&session, &uri, &config, &reporter))
        });

        match handle.await {
            Ok(Ok(())) => tracing::info!("<<< SUCCESS {}", task_id),
            Ok(Err(e)) => tracing::error!("Process error: {e}"),
            Err(e) => tracing::error!("Block join: {e}"),
        }

        Some(())
    }

    pub async fn is_empty(&self) -> bool {
        let len = self.tasks.read().await.len();
        tracing::debug!("is_empty check: {} tasks", len);
        len == 0
    }

    pub fn run_worker(self: Arc<Self>, idle_timeout: Duration) -> JoinHandle<()> {
        let queue_clone = Arc::clone(&self);
        tokio::spawn(async move {
            let mut last_task = Instant::now();
            loop {
                if queue_clone.is_empty().await {
                    if last_task.elapsed() > idle_timeout {
                        tracing::info!("Idle timeout, worker exiting");
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    continue;
                }
                if queue_clone.process_next().await.is_some() {
                    last_task = Instant::now();
                    tracing::info!("Worker: task done");
                }
            }
        })
    }

    pub fn progress_tx(&self) -> tokio::sync::broadcast::Sender<ProgressUpdate> {
        self.progress_tx.clone()
    }
}
