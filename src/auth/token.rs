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
    token_path: String,
    current_token: Arc<RwLock<Option<super::oauth::TokenData>>>,
}

impl TokenRefresher {
    /// Create a new token refresher (internal use only)
    pub(crate) fn new(token_path: String, initial_token: super::oauth::TokenData) -> Self {
        Self {
            token_path,
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
pub(crate) fn read_token_data(token_path: &str) -> Option<super::oauth::TokenData> {
    let content = std::fs::read_to_string(token_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save token data to file
pub(crate) fn save_token_data(token_path: &str, token_data: &super::oauth::TokenData) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(token_data)?;
    std::fs::write(token_path, json)?;
    Ok(())
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
    fn test_save_and_read_token_data() {
        let temp_file = "/tmp/test_token_data.json";
        let token_data = crate::auth::oauth::TokenData {
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
    }    #[test]
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
}