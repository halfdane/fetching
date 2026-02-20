pub mod auth;
pub mod cache;
pub mod config;
pub mod error;
pub mod implementations;
pub mod input;
pub mod m3u;
pub mod metadata;
pub mod playback;
pub mod processor;
pub mod stream;
pub mod traits;
mod progress;
pub mod queue;
pub use queue::{SharedQueue, Task};

pub use crate::progress::{ProgressScope, ProgressUpdate, init_progress_tx};
// Re-export create_session
pub use auth::session::create_session;
pub use progress::PROGRESS_TX;
