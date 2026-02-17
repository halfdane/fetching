// Remove duplicate imports; use fully qualified paths or module imports as needed.

// Removed duplicate process_url_from_args; only process_url is required by spec.
// Library interface for integration tests
use uuid::Uuid;
use anyhow;
use tokio::sync::{mpsc, broadcast};
use tokio::task::JoinHandle;
pub mod auth;
pub mod cache;
pub mod config;
pub mod input;
pub mod processor;
pub mod m3u;
pub mod error;
pub mod traits;
pub mod implementations;
pub mod metadata;
pub mod stream;
pub mod playback;
use std::sync::Arc;

// Re-export create_session if needed
pub use auth::session::create_session;


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressUpdate {
    pub task_id: Uuid,
    pub scope: ProgressScope,
    pub status: String,
    pub current: u32,
    pub total: u32,
    pub item: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ProgressScope {
    Track,
    Album,
    Playlist,
    Global,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub task_id: Uuid,
    pub uri: String,
}


pub async fn process_url_safe(
    session: &librespot_core::Session,
    task_id: Uuid, uri: &str, config: &config::Config,
    tx: broadcast::Sender<ProgressUpdate>,
) -> Result<(), anyhow::Error> {
    tokio::task::spawn_blocking({
        let session = session.clone();
        let uri = uri.to_string();
        let config = config.clone();
        let tx = tx.clone();
        move || {
            // Runs in blocking thread - safe for librespot decoder
            tokio::runtime::Handle::current()
                .block_on(async move {
                    processor::process_url(&session, task_id, &uri, &config, tx).await
                })
        }
    }).await??;
    Ok(())
}

pub fn spawn_task_processor(
    session: &librespot_core::session::Session,
    task_rx: mpsc::Receiver<Task>,
    tx: tokio::sync::broadcast::Sender<ProgressUpdate>,
) -> JoinHandle<bool> {
    let session_clone = session.clone();  // Your original
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut task_rx = task_rx;
        let mut any_error = false;
        while let Some(task) = task_rx.recv().await {
            tracing::info!("Processing task {}: {}", task.task_id, task.uri);
            let uri = task.uri.clone();
            let config = config::Config::from_env();
                        if let Err(e) = process_url_safe(&session_clone, task.task_id, &uri, &config, tx_clone.clone()).await {
                eprintln!("Error processing {}: {e}", task.uri);
                any_error = true;
            }
        }
        any_error
    })
}

pub async fn queue_uri_tasks(
    uris: Vec<String>,
    task_tx: mpsc::Sender<Task>,
) -> anyhow::Result<()> {
    for uri in uris {
        let task_id = Uuid::new_v4();
        let task = Task { task_id, uri };
        tracing::info!("Queueing task {}: {}", task.task_id, task.uri);
        task_tx.send(task).await.map_err(|_| anyhow::anyhow!("Queue send failed"))?;
    }
    Ok(())
}
