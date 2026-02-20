//! Spotify authentication and session management.
//!
//! Handles OAuth token acquisition, storage, and automatic session creation.
//! Tokens are cached to disk and reused across runs. Invalid tokens trigger
//! automatic re-authentication via browser-based OAuth flow.
//!
//! Supports automatic token refresh using refresh tokens to avoid repeated
//! browser-based OAuth flows.

pub mod oauth;
pub mod session;
pub mod token;

// Re-exports for public API
pub use session::create_session_with_auto_refresh;
pub use token::TokenRefresher;

use librespot_core::authentication::Credentials;
use tracing::{info, warn};

use self::oauth::{perform_oauth_flow, refresh_access_token};
use self::token::{is_token_expired, read_token_data, save_token_data};

/// Handles loading, validating, and refreshing the access token
pub async fn get_credentials(
    token_path: &str,
) -> anyhow::Result<Credentials> {
    // Validate OAuth configuration first
    oauth::validate_oauth_config()?;

    let cache = librespot_core::cache::Cache::new(Some(".cache"), Some(".cache"), Some(".cache/files"), None)?;
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
                        Credentials::with_access_token(
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
                Credentials::with_access_token(
                    token_data.access_token.trim().to_string(),
                ),
            );
        }
    }

    // No valid cached token, do full OAuth flow
    let token_data = perform_oauth_flow(token_path).await?;

    Ok(
        Credentials::with_access_token(
            token_data.access_token.trim(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_get_credentials_with_valid_token_file() {
        // Create a temporary token file with valid token
        let temp_file = NamedTempFile::new().unwrap();
        let token_path = temp_file.path().to_str().unwrap();

        let token_data = oauth::TokenData {
            access_token: "valid_token".to_string(),
            refresh_token: "refresh_token".to_string(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() + 3600, // Expires in 1 hour
        };

        token::save_token_data(token_path, &token_data).unwrap();

        // Should return credentials from the token file
        let result = get_credentials(token_path).await;
        assert!(result.is_ok());

        // We successfully got credentials (don't need to inspect the exact type)
        let _creds = result.unwrap();
    }
}