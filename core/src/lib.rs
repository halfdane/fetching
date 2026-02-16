// Remove duplicate imports; use fully qualified paths or module imports as needed.

// Removed duplicate process_url_from_args; only process_url is required by spec.
// Library interface for integration tests
use uuid::Uuid;
use serde::Serialize;
use librespot_core::Session;
use anyhow;
use tokio::sync::mpsc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
pub mod auth;
pub mod cache;
pub mod cli;
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

#[derive(Debug)]
pub struct Task {
    pub task_id: Uuid,
    pub uri: String,
}


pub async fn process_url(task_id: Uuid, url: String, tx: tokio::sync::broadcast::Sender<ProgressUpdate>) -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::from_env();
    let token_path = ".spotify_access_token";
    let (session, _refresher, _refresh_handle) = auth::session::create_session(token_path).await?;
    processor::process_url(&session, task_id, &url, &config, tx).await?;
    Ok(())
}

pub async fn process_single_url(session: &librespot_core::session::Session, task_id: Uuid, url: String, tx: tokio::sync::broadcast::Sender<ProgressUpdate>) -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::from_env();
    processor::process_url(session, task_id, &url, &config, tx).await?;
    Ok(())
}

pub async fn process_uris(uris: &[String], tx: tokio::sync::broadcast::Sender<ProgressUpdate>) -> Result<(), Box<dyn std::error::Error>> {
    // For each URL, generate a task_id and call process_url
    for url in uris {
        let task_id = Uuid::new_v4();
        process_url(task_id, url.clone(), tx.clone()).await?;
    }
    Ok(())
}

pub fn spawn_task_processor(
    session: &librespot_core::session::Session,
    task_rx: mpsc::Receiver<Task>,
    tx: tokio::sync::broadcast::Sender<ProgressUpdate>,
) -> JoinHandle<bool> {
    let session_clone = session.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut task_rx = task_rx;
        let mut any_error = false;
        while let Some(task) = task_rx.recv().await {
            let uri = task.uri.clone();
            if let Err(e) = process_single_url(&session_clone, task.task_id, uri, tx_clone.clone()).await {
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
        task_tx.send(task).await.map_err(|_| anyhow::anyhow!("Queue send failed"))?;
    }
    Ok(())
}
