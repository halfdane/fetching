// Remove duplicate imports; use fully qualified paths or module imports as needed.

// Removed duplicate process_url_from_args; only process_url is required by spec.
// Library interface for integration tests
use uuid::Uuid;
use serde::Serialize;
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
pub mod mocks;
pub mod stream;
pub mod playback;

// Re-export create_session if needed
pub use auth::session::create_session;

#[derive(Serialize)]
pub struct ProgressUpdate {
    pub task_id: Uuid,
    pub status: String,
    pub percent: u8,
    pub item: String,
}

pub async fn process_uris(uris: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration from environment variables
    let config = config::Config::from_env();

    let token_path = ".spotify_access_token";
    let (session, _refresher, _refresh_handle) = auth::session::create_session(token_path).await?;

	processor::process_uris(&session, &uris, &config).await?;

    Ok(())
}
