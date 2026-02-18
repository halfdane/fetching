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

// Re-export for public API
pub use session::create_session;
pub use token::TokenRefresher;

use librespot_core::authentication::Credentials;
use tracing::{info, warn};

use self::oauth::{perform_oauth_flow, refresh_access_token};
use self::token::{is_token_expired, read_token_data};

/// Handles loading, validating, and refreshing the access token
pub struct CredentialsWithTokenData {
    pub credentials: Credentials,
    pub token_data: Option<crate::auth::oauth::TokenData>,
}

pub async fn get_credentials(
    credentials_path: &str,
) -> anyhow::Result<CredentialsWithTokenData> {
    // Validate OAuth configuration first
    oauth::validate_oauth_config()?;

    let cache = librespot_core::cache::Cache::new(Some(".cache"), Some(".cache"), Some(".cache/files"), None)?;
    if let Some(creds) = cache.credentials() {
        tracing::debug!("get_credentials: cache hit, returning credentials from cache");
        return Ok(CredentialsWithTokenData { credentials: creds, token_data: None });
    }

    // Try to read token data from file
    if let Some(token_data) = read_token_data(credentials_path) {
        tracing::debug!("get_credentials: found token file");
        // Check if token is expired
        if is_token_expired(token_data.expires_at) {
            tracing::debug!("get_credentials: token expired, attempting refresh");
            info!("Access token expired, attempting refresh...");
            // Try to refresh the token
            match refresh_access_token(&token_data.refresh_token).await {
                Ok(new_token_data) => {
                    tracing::debug!("get_credentials: token refresh successful");
                    info!("Token refreshed successfully");
                    // Do NOT save refreshed credentials to file. Only update in-memory token.
                    // If a new refresh token is issued, warn user (already handled in refresher).
                    return Ok(CredentialsWithTokenData {
                        credentials: Credentials::with_access_token(new_token_data.access_token.trim()),
                        token_data: Some(new_token_data),
                    });
                }
                Err(e) => {
                    tracing::debug!("get_credentials: token refresh failed: {}", e);
                    warn!("Token refresh failed: {}, will re-authenticate", e);
                    // Fall through to full OAuth flow
                }
            }
        } else {
            tracing::debug!("get_credentials: token file valid, using token");
            // Token is still valid
            return Ok(CredentialsWithTokenData {
                credentials: Credentials::with_access_token(token_data.access_token.trim().to_string()),
                token_data: Some(token_data),
            });
        }
    }

    // No valid cached token, do full OAuth flow
    tracing::debug!("get_credentials: performing OAuth flow");
    let token_data = perform_oauth_flow(credentials_path).await?;
    tracing::debug!("get_credentials: OAuth flow complete, returning credentials");
    Ok(CredentialsWithTokenData {
        credentials: Credentials::with_access_token(token_data.access_token.trim()),
        token_data: Some(token_data),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::NamedTempFile;
}