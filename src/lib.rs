// Library interface for integration tests

pub mod auth;
pub mod cache;
pub mod cli;
pub mod config;
pub mod error;
pub mod implementations;
pub mod input;
pub mod metadata;
pub mod mocks;
pub mod m3u;
pub mod playback;
pub mod processor;
pub mod stream;
pub mod traits;


use std::error::Error;
/// Public async function for URL processing (implementation will be moved from main.rs)
pub async fn process_url(url: &str) -> Result<(), Box<dyn Error>> {
	// Implementation will be moved from main.rs
	Ok(())
}

// Re-export authentication/session helpers for main.rs and tests
pub use crate::auth::session::{create_session_with_auto_refresh, create_authenticated_session};
pub use crate::auth::get_credentials;
// The following re-exports require the items to be pub in their module:
// pub use crate::auth::token::{TokenRefresher, read_token_data, save_token_data, is_token_expired};
