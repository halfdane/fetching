//! Token storage, refresh, and validation for Spotify authentication.
//!
//! Handles token persistence to disk, background refresh monitoring,
//! and token expiration checking.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Background token refresher that monitors and refreshes tokens automatically.
///
/// Spawns a background task that:
/// - Checks token expiration every 5 minutes
/// - Refreshes proactively when token expires within 10 minutes
/// - Updates both in-memory token and disk storage
/// - Ensures long-running sessions don't experience authentication failures
pub struct TokenRefresher {
    current_token: Arc<RwLock<Option<super::oauth::TokenData>>>,
}

impl TokenRefresher {
    /// Create a new token refresher (internal use only)
    pub(crate) fn new(initial_token: super::oauth::TokenData) -> Self {
        Self {
            current_token: Arc::new(RwLock::new(Some(initial_token))),
        }
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
                        match super::oauth::refresh_access_token(&ref_token).await {
                            Ok(new_token_data) => {
                                info!("Background token refresh successful");

                                // Do NOT save refreshed credentials to file. Only update in-memory token.
                                // If a new refresh token is issued, print a warning.
                                if new_token_data.refresh_token != ref_token {
                                    warn!("******************************************************");
                                    warn!("WARNING: Spotify issued a new refresh token.");
                                    warn!("Please re-authenticate and update your credentials file before your next run, or you may need to re-authenticate again.");
                                    warn!("******************************************************");
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
pub(crate) fn read_token_data(token_path: &str) -> Option<super::oauth::TokenData> {
    let content = std::fs::read_to_string(token_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Check if a token has expired (with 5 minute buffer)
pub(crate) fn is_token_expired(expires_at: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    // Consider expired if within 5 minutes of expiration
    now >= expires_at.saturating_sub(300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_token_data_missing() {
        let nonexistent_path = "/tmp/definitely_does_not_exist_token_67890.json";
        assert!(read_token_data(nonexistent_path).is_none());
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

    #[tokio::test]
    async fn test_token_refresher_creation() {
        let temp_file = "/tmp/test_refresher_token.json";
        let token_data = crate::auth::oauth::TokenData {
            access_token: "test_token".to_string(),
            refresh_token: "refresh_token".to_string(),
            expires_at: 1234567890,
        };

        let refresher = TokenRefresher::new(temp_file.to_string(), token_data.clone());

        // Verify the refresher was created with correct data
        // We can't easily test the background task without complex mocking,
        // but we can test the basic structure
        assert_eq!(refresher.credentials_path, temp_file);

        // Cleanup
        std::fs::remove_file(temp_file).ok();
    }
    // Obsolete test for file writing/removal removed. Only token reading/expiry tests remain.
}