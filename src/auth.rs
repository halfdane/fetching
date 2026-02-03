//! Spotify authentication and session management.
//!
//! Handles OAuth token acquisition, storage, and automatic session creation.
//! Tokens are cached to disk and reused across runs. Invalid tokens trigger
//! automatic re-authentication via browser-based OAuth flow.
//!
//! Supports automatic token refresh using refresh tokens to avoid repeated
//! browser-based OAuth flows.

use anyhow::Context;
use librespot_core::{cache::Cache, config::SessionConfig, session::Session};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, info, warn};

const KEYMASTER_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const SCOPES: &[&str] = &["streaming"];

/// Stored token data with refresh capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TokenData {
    access_token: String,
    refresh_token: String,
    expires_at: u64, // Unix timestamp in seconds
}

/// Background token refresher that monitors and refreshes tokens automatically.
///
/// Spawns a background task that:
/// - Checks token expiration every 5 minutes
/// - Refreshes proactively when token expires within 10 minutes
/// - Updates both in-memory token and disk storage
/// - Ensures long-running sessions don't experience authentication failures
pub struct TokenRefresher {
    token_path: String,
    current_token: Arc<RwLock<Option<TokenData>>>,
}

impl TokenRefresher {
    /// Create a new token refresher (internal use only)
    pub(crate) fn new(token_path: String, initial_token: TokenData) -> Self {
        Self {
            token_path,
            current_token: Arc::new(RwLock::new(Some(initial_token))),
        }
    }

    /// Get current access token (for future use)
    #[allow(dead_code)]
    pub async fn get_access_token(&self) -> Option<String> {
        let token = self.current_token.read().await;
        token.as_ref().map(|t| t.access_token.clone())
    }

    /// Start background refresh task that monitors token expiration
    /// Checks every 5 minutes and refreshes if token expires within 10 minutes
    pub fn start_background_refresh(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                // Check every 5 minutes
                sleep(Duration::from_secs(300)).await;

                let should_refresh = {
                    let token = self.current_token.read().await;
                    if let Some(ref token_data) = *token {
                        // Refresh if expires within 10 minutes
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or(Duration::ZERO)
                            .as_secs();
                        token_data.expires_at <= now + 600
                    } else {
                        false
                    }
                };

                if should_refresh {
                    debug!("Token expiring soon, refreshing in background...");
                    
                    let refresh_token = {
                        let token = self.current_token.read().await;
                        token.as_ref().map(|t| t.refresh_token.clone())
                    };

                    if let Some(ref_token) = refresh_token {
                        match refresh_access_token(&ref_token).await {
                            Ok(new_token_data) => {
                                info!("Background token refresh successful");
                                
                                // Save to file
                                if let Err(e) = save_token_data(&self.token_path, &new_token_data) {
                                    warn!("Failed to save refreshed token: {}", e);
                                }

                                // Update in-memory token
                                let mut token = self.current_token.write().await;
                                *token = Some(new_token_data);
                            }
                            Err(e) => {
                                warn!("Background token refresh failed: {}", e);
                            }
                        }
                    }
                }
            }
        })
    }
}

/// Read token data from file, returning None if file doesn't exist or is invalid
fn read_token_data(token_path: &str) -> Option<TokenData> {
    let content = std::fs::read_to_string(token_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save token data to file
fn save_token_data(token_path: &str, token_data: &TokenData) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(token_data)?;
    std::fs::write(token_path, json)?;
    Ok(())
}

/// Check if a token has expired (with 5 minute buffer)
fn is_token_expired(expires_at: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    // Consider expired if within 5 minutes of expiration
    now >= expires_at.saturating_sub(300)
}

/// Validate that OAuth constants are properly configured
pub fn validate_oauth_config() -> anyhow::Result<()> {
    if KEYMASTER_CLIENT_ID.is_empty() {
        anyhow::bail!("KEYMASTER_CLIENT_ID cannot be empty");
    }
    if REDIRECT_URI.is_empty() {
        anyhow::bail!("REDIRECT_URI cannot be empty");
    }
    if SCOPES.is_empty() {
        anyhow::bail!("SCOPES cannot be empty");
    }
    Ok(())
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
    let credentials = get_credentials(token_path).await?;
    let session = create_authenticated_session(credentials).await?;

    // Get current token data for refresher
    let token_data = read_token_data(token_path)
        .ok_or_else(|| anyhow::anyhow!("Failed to read token data after authentication"))?;

    let refresher = Arc::new(TokenRefresher::new(token_path.to_string(), token_data));
    let refresh_handle = Arc::clone(&refresher).start_background_refresh();

    info!("Background token refresh task started");

    Ok((session, refresher, refresh_handle))
}

/// Handles loading, validating, and refreshing the access token
pub async fn get_credentials(
    token_path: &str,
) -> anyhow::Result<librespot_core::authentication::Credentials> {
    // Validate OAuth configuration first
    validate_oauth_config()?;

    let cache = Cache::new(Some(".cache"), Some(".cache"), Some(".cache/files"), None)?;
    if let Some(creds) = cache.credentials() {
        return Ok(creds);
    }

    // Try to read token data from file
    if let Some(token_data) = read_token_data(token_path) {
        // Check if token is expired
        if is_token_expired(token_data.expires_at) {
            info!("Access token expired, attempting refresh...");
            
            // Try to refresh the token
            match refresh_access_token(&token_data.refresh_token).await {
                Ok(new_token_data) => {
                    info!("Token refreshed successfully");
                    if let Err(e) = save_token_data(token_path, &new_token_data) {
                        warn!("Failed to save refreshed token: {}", e);
                    }
                    return Ok(
                        librespot_core::authentication::Credentials::with_access_token(
                            new_token_data.access_token.trim(),
                        ),
                    );
                }
                Err(e) => {
                    warn!("Token refresh failed: {}, will re-authenticate", e);
                    // Fall through to full OAuth flow
                }
            }
        } else {
            // Token is still valid
            return Ok(
                librespot_core::authentication::Credentials::with_access_token(
                    token_data.access_token.trim().to_string(),
                ),
            );
        }
    }

    // No valid cached token, do full OAuth flow
    info!("No valid access token found, starting OAuth flow...");
    let oauth_token = librespot_oauth::OAuthClientBuilder::new(
        KEYMASTER_CLIENT_ID,
        REDIRECT_URI,
        SCOPES.to_vec(),
    )
    .open_in_browser()
    .build()
    .context("Failed to build OAuth client")?
    .get_access_token()
    .context("Failed to get access token")?;

    // Convert Instant to Unix timestamp
    // oauth_token.expires_at is an Instant representing when token expires
    let seconds_until_expiry = oauth_token
        .expires_at
        .checked_duration_since(std::time::Instant::now())
        .map(|d| d.as_secs())
        .unwrap_or(3600); // Default to 1 hour if calculation fails

    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        + seconds_until_expiry;

    let token_data = TokenData {
        access_token: oauth_token.access_token.clone(),
        refresh_token: oauth_token.refresh_token.clone(),
        expires_at,
    };

    if let Err(e) = save_token_data(token_path, &token_data) {
        warn!("Failed to save token data: {}", e);
    } else {
        info!("Token data saved to {}", token_path);
    }

    Ok(
        librespot_core::authentication::Credentials::with_access_token(
            oauth_token.access_token.trim(),
        ),
    )
}

/// Refresh an expired access token using a refresh token
async fn refresh_access_token(refresh_token: &str) -> anyhow::Result<TokenData> {
    let oauth_client = librespot_oauth::OAuthClientBuilder::new(
        KEYMASTER_CLIENT_ID,
        REDIRECT_URI,
        SCOPES.to_vec(),
    )
    .build()
    .context("Failed to build OAuth client")?;

    let oauth_token = oauth_client
        .refresh_token_async(refresh_token)
        .await
        .context("Failed to refresh token")?;

    // Convert Instant to Unix timestamp
    // oauth_token.expires_at is an Instant representing when token expires
    let seconds_until_expiry = oauth_token
        .expires_at
        .checked_duration_since(std::time::Instant::now())
        .map(|d| d.as_secs())
        .unwrap_or(3600); // Default to 1 hour if calculation fails

    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        + seconds_until_expiry;

    Ok(TokenData {
        access_token: oauth_token.access_token,
        refresh_token: oauth_token.refresh_token,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_oauth_config() {
        // Should pass with valid constants
        assert!(validate_oauth_config().is_ok());
    }

    #[test]
    fn test_oauth_constants_are_set() {
        assert!(!KEYMASTER_CLIENT_ID.is_empty());
        assert!(!REDIRECT_URI.is_empty());
        assert!(!SCOPES.is_empty());

        // Verify specific values
        assert_eq!(KEYMASTER_CLIENT_ID, "65b708073fc0480ea92a077233ca87bd");
        assert_eq!(REDIRECT_URI, "http://127.0.0.1:8898/login");
        assert_eq!(SCOPES, &["streaming"]);
    }

    #[test]
    fn test_read_token_data_missing() {
        let nonexistent_path = "/tmp/definitely_does_not_exist_token_67890.json";
        assert!(read_token_data(nonexistent_path).is_none());
    }

    #[test]
    fn test_save_and_read_token_data() {
        let temp_file = "/tmp/test_token_data.json";
        let token_data = TokenData {
            access_token: "test_access_token".to_string(),
            refresh_token: "test_refresh_token".to_string(),
            expires_at: 1234567890,
        };

        // Save and read back
        save_token_data(temp_file, &token_data).unwrap();
        let result = read_token_data(temp_file);
        assert!(result.is_some());
        
        let loaded = result.unwrap();
        assert_eq!(loaded.access_token, "test_access_token");
        assert_eq!(loaded.refresh_token, "test_refresh_token");
        assert_eq!(loaded.expires_at, 1234567890);

        // Cleanup
        std::fs::remove_file(temp_file).ok();
    }

    #[test]
    fn test_is_token_expired() {
        // Token expired 1 hour ago
        let past = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(3600);
        assert!(is_token_expired(past));

        // Token expires in 1 hour (should not be considered expired)
        let future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        assert!(!is_token_expired(future));

        // Token expires in 4 minutes (within 5 minute buffer, should be expired)
        let soon = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 240;
        assert!(is_token_expired(soon));
    }

    #[test]
    fn test_redirect_uri_format() {
        // Verify redirect URI is a valid URL format
        assert!(REDIRECT_URI.starts_with("http://") || REDIRECT_URI.starts_with("https://"));
        assert!(REDIRECT_URI.contains("127.0.0.1") || REDIRECT_URI.contains("localhost"));
    }

    #[test]
    fn test_scopes_contains_streaming() {
        assert!(SCOPES.contains(&"streaming"));
    }

    #[tokio::test]
    async fn test_token_refresher_get_access_token() {
        let token_data = TokenData {
            access_token: "test_access".to_string(),
            refresh_token: "test_refresh".to_string(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 3600,
        };

        let refresher = TokenRefresher::new("/tmp/test_token.json".to_string(), token_data);
        let access_token = refresher.get_access_token().await;
        
        assert!(access_token.is_some());
        assert_eq!(access_token.unwrap(), "test_access");
    }
}
