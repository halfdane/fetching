use once_cell::sync::OnceCell;
use tokio::sync::broadcast;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressUpdate {
    pub task_id: uuid::Uuid,
    pub scope: ProgressScope,
    pub status: String,
    pub current: u32,
    pub total: u32,
    pub user_visible_identifier: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ProgressScope {
    Track,
    Album,
    Playlist,
    Global,
}

pub static PROGRESS_TX: OnceCell<broadcast::Sender<ProgressUpdate>> = OnceCell::new();

pub fn init_progress_tx(capacity: usize) -> broadcast::Receiver<ProgressUpdate> {
    let (tx, rx) = broadcast::channel(capacity);
    PROGRESS_TX.set(tx).unwrap();
    rx
}

pub fn send_update(update: ProgressUpdate) {
    if let Some(tx) = PROGRESS_TX.get() {
        let _ = tx.send(update);
    }
}
