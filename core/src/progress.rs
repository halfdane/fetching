use once_cell::sync::OnceCell;
use tokio::sync::broadcast;
use crate::ProgressUpdate;

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
