//! Session creation and management for Spotify authentication.
//!
//! Handles creating authenticated Spotify sessions with automatic
//! token refresh capabilities for long-running operations.
//!
//! All references to token_path have been renamed to credentials_path for clarity and consistency with librespot.

use librespot_core::{cache::Cache, config::SessionConfig, session::Session};
use std::sync::Arc;
use tracing::info;

use super::token::TokenRefresher;

/// Create a Spotify session with automatic background token refresh.
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
/// use fetching_core::auth::create_session;
/// # async fn example() -> anyhow::Result<()> {
/// let (session, _refresher, _handle) = create_session(".spotify_access_token").await?;
/// // Use session for hours without worrying about token expiration
/// # Ok(())
/// # }
/// ```
pub async fn create_session(
    credentials_path: &str,
) -> anyhow::Result<(Session, Arc<TokenRefresher>, tokio::task::JoinHandle<()>)> {
    let credentials = super::get_credentials(credentials_path).await?;
    let session = create_authenticated_session(credentials.credentials.clone()).await?;

    // Get current token data for refresher
    // Prefer file-based token data, else use in-memory token data from OAuth
    let token_data = match super::token::read_token_data(credentials_path) {
        Some(data) => data,
        None => {
            if let Some(token_data) = credentials.token_data.clone() {
                tracing::warn!(
                    "\nWARNING: Credentials file not found or unreadable after authentication. Using in-memory credentials for this session.\n\
    Token refresh will NOT work in future runs until you provide a credentials file with a refresh token.\n\
    Please ensure you have copied the credentials to the specified file for future runs.\n"
                );
                token_data
            } else {
                anyhow::bail!("No usable token data available for refresher");
            }
        }
    };

    let refresher = Arc::new(TokenRefresher::new(token_data));
    let refresh_handle = Arc::clone(&refresher).start_background_refresh();

    info!("Background token refresh task started");

    Ok((session, refresher, refresh_handle))
}

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
