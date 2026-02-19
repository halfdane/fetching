//! OAuth flow and configuration for Spotify authentication.
//!
//! Handles OAuth client setup, token acquisition via browser flow,
//! and token refresh operations.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::info;

const KEYMASTER_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const SCOPES: &[&str] = &["streaming"];

/// Stored token data with refresh capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at: u64, // Unix timestamp in seconds
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

/// Refresh an expired access token using a refresh token
pub(crate) async fn refresh_access_token(refresh_token: &str) -> anyhow::Result<TokenData> {
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

/// Perform full OAuth flow to acquire new tokens
pub(crate) async fn perform_oauth_flow(credentials_path: &str) -> anyhow::Result<TokenData> {
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

    // Instead of saving to file, display credentials to user for manual storage
    println!("\n******************************************************");
    println!("Spotify authentication successful!");
    println!(
        "Please copy the following credentials and save them to a file (e.g., {}), then re-run the program with --credentials-file.",
        credentials_path
    );
    println!("\n----- BEGIN SPOTIFY CREDENTIALS -----");
    println!("{}", serde_json::to_string_pretty(&token_data).unwrap());
    println!("----- END SPOTIFY CREDENTIALS -----\n");
    println!("******************************************************\n");

    // Wait for user to press Enter before continuing
    use std::io::{self, Write};
    print!("Press Enter to continue...");
    io::stdout().flush().unwrap();
    let mut _input = String::new();
    io::stdin().read_line(&mut _input).unwrap();

    Ok(token_data)
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
    fn test_redirect_uri_format() {
        // Verify redirect URI is a valid URL format
        assert!(REDIRECT_URI.starts_with("http://") || REDIRECT_URI.starts_with("https://"));
        assert!(REDIRECT_URI.contains("127.0.0.1") || REDIRECT_URI.contains("localhost"));
    }

    #[test]
    fn test_scopes_contains_streaming() {
        assert!(SCOPES.contains(&"streaming"));
    }
}
