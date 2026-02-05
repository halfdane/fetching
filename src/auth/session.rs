//! Session creation and management for Spotify authentication.
//!
//! Handles creating authenticated Spotify sessions with automatic
//! token refresh capabilities for long-running operations.

use librespot_core::{cache::Cache, config::SessionConfig, session::Session};
use std::sync::Arc;
use tracing::info;

use super::token::TokenRefresher;

/// Handles session creation and authentication, including re-auth if needed
pub async fn create_authenticated_session(
    credentials: librespot_core::authentication::Credentials,
) -> anyhow::Result<Session> {
    let cache = Cache::new(Some(".cache"), Some(".cache"), Some(".cache/files"), None)?;
    let session = Session::new(SessionConfig::default(), Some(cache));
    // Try to connect
    session.connect(credentials, false).await?;
    Ok(session)
}

/// Create session with automatic background token refresh
///
/// This is the recommended way to create a session for long-running operations.
/// Returns a tuple of:
/// - `Session`: The authenticated Spotify session
/// - `Arc<TokenRefresher>`: Handle to query current token (optional use)
/// - `JoinHandle`: Background task handle (kept alive for duration of app)
///
/// The background task will automatically refresh the token before expiration,
/// ensuring uninterrupted operation during long album/playlist downloads.
///
/// # Example
/// ```no_run
/// use spotify_player::auth::create_session_with_auto_refresh;
/// # async fn example() -> anyhow::Result<()> {
/// let (session, _refresher, _handle) = create_session_with_auto_refresh(".spotify_access_token").await?;
/// // Use session for hours without worrying about token expiration
/// # Ok(())
/// # }
/// ```
pub async fn create_session_with_auto_refresh(
    token_path: &str,
) -> anyhow::Result<(Session, Arc<TokenRefresher>, tokio::task::JoinHandle<()>)> {
    let credentials = super::get_credentials(token_path).await?;
    let session = create_authenticated_session(credentials).await?;

    // Get current token data for refresher
    let token_data = super::token::read_token_data(token_path)
        .ok_or_else(|| anyhow::anyhow!("Failed to read token data after authentication"))?;

    let refresher = Arc::new(TokenRefresher::new(token_path.to_string(), token_data));
    let refresh_handle = Arc::clone(&refresher).start_background_refresh();

    info!("Background token refresh task started");

    Ok((session, refresher, refresh_handle))
}