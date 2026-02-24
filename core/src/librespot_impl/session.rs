//! Spotify session creation with automatic token refresh.
//!
//! Single entry point: [`create_session`].
//! Handles token persistence, expiry checking, silent refresh, full OAuth
//! re-authentication, and spawning a background refresh task — all internally.

use anyhow::Context;
use librespot_core::{authentication::Credentials, cache::Cache, config::SessionConfig, Session};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{debug, info, warn};

// ── Constants ─────────────────────────────────────────────────────────────────

const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const SCOPES: &[&str] = &["streaming"];

/// Proactively refresh when fewer than this many seconds remain.
const REFRESH_THRESHOLD_SECS: u64 = 600;
/// How often the background task checks token expiry.
const REFRESH_POLL_INTERVAL_SECS: u64 = 300;

// ── Token storage ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenData {
    access_token: String,
    refresh_token: String,
    /// Unix timestamp (seconds) at which the token expires.
    expires_at: u64,
}

fn read_token(path: &str) -> Option<TokenData> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_token(path: &str, token: &TokenData) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(token)?)?;
    Ok(())
}

fn is_expiring_soon(expires_at: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    now >= expires_at.saturating_sub(REFRESH_THRESHOLD_SECS)
}

// ── OAuth operations ──────────────────────────────────────────────────────────

async fn refresh_token(token: &TokenData) -> anyhow::Result<TokenData> {
    let client = librespot_oauth::OAuthClientBuilder::new(CLIENT_ID, REDIRECT_URI, SCOPES.to_vec())
        .build()
        .context("Failed to build OAuth client for token refresh")?;

    let new_token = client
        .refresh_token_async(&token.refresh_token)
        .await
        .context("Token refresh failed")?;

    let seconds_until_expiry = new_token
        .expires_at
        .checked_duration_since(Instant::now())
        .map(|d| d.as_secs())
        .unwrap_or(3600);

    Ok(TokenData {
        access_token: new_token.access_token,
        refresh_token: new_token.refresh_token,
        expires_at: now_secs() + seconds_until_expiry,
    })
}

async fn oauth_flow(token_path: &str) -> anyhow::Result<TokenData> {
    info!("No valid token found — starting browser OAuth flow");
    let client = librespot_oauth::OAuthClientBuilder::new(CLIENT_ID, REDIRECT_URI, SCOPES.to_vec())
        .open_in_browser()
        .build()
        .context("Failed to build OAuth client for interactive flow")?;

    let token = client
        .get_access_token()
        .context("OAuth flow failed to obtain access token")?;

    let seconds_until_expiry = token
        .expires_at
        .checked_duration_since(Instant::now())
        .map(|d| d.as_secs())
        .unwrap_or(3600);

    let token_data = TokenData {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: now_secs() + seconds_until_expiry,
    };

    save_token(token_path, &token_data).unwrap_or_else(|e| warn!("Could not save token: {e}"));
    Ok(token_data)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

// ── Session creation ──────────────────────────────────────────────────────────

fn make_cache() -> anyhow::Result<Cache> {
    Cache::new(Some(".cache"), Some(".cache"), Some(".cache/files"), None)
        .context("Failed to create librespot cache")
}

async fn resolve_credentials(
    token_path: &str,
    cache: &Cache,
) -> anyhow::Result<(Credentials, TokenData)> {
    // 1. Try cached credentials from librespot's own cache
    if let Some(creds) = cache.credentials() {
        if let Some(token) = read_token(token_path) {
            if !is_expiring_soon(token.expires_at) {
                return Ok((creds, token));
            }
        }
    }

    // 2. Try reading a stored token from disk
    if let Some(token) = read_token(token_path) {
        if is_expiring_soon(token.expires_at) {
            debug!("Token expiring soon, refreshing before connecting");
            match refresh_token(&token).await {
                Ok(new_token) => {
                    save_token(token_path, &new_token)
                        .unwrap_or_else(|e| warn!("Could not save refreshed token: {e}"));
                    let creds = Credentials::with_access_token(new_token.access_token.trim());
                    return Ok((creds, new_token));
                }
                Err(e) => warn!("Pre-connect refresh failed ({e}), trying existing token"),
            }
        }
        let creds = Credentials::with_access_token(token.access_token.trim());
        return Ok((creds, token));
    }

    // 3. Full OAuth browser flow
    let token = oauth_flow(token_path).await?;
    let creds = Credentials::with_access_token(token.access_token.trim());
    Ok((creds, token))
}

async fn connect(credentials: Credentials, cache: Cache) -> anyhow::Result<Session> {
    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, false).await?;
    Ok(session)
}

fn spawn_background_refresh(token_path: String, initial_token: TokenData) {
    tokio::spawn(async move {
        let mut token = initial_token;
        loop {
            sleep(Duration::from_secs(REFRESH_POLL_INTERVAL_SECS)).await;
            if is_expiring_soon(token.expires_at) {
                debug!("Background refresh: token expiring soon");
                match refresh_token(&token).await {
                    Ok(new_token) => {
                        save_token(&token_path, &new_token)
                            .unwrap_or_else(|e| warn!("Could not save background token: {e}"));
                        info!("Background token refresh successful");
                        token = new_token;
                    }
                    Err(e) => warn!("Background token refresh failed: {e}"),
                }
            }
        }
    });
}

/// Create an authenticated Spotify session.
///
/// Handles the full authentication lifecycle:
/// - Reuses a cached token if still valid
/// - Silently refreshes the token if it is about to expire
/// - Opens a browser OAuth flow if no usable token exists
/// - Removes a stale token file and retries once on bad-credentials errors
/// - Spawns a background task to keep the token fresh for long-running sessions
pub async fn create_session(token_path: &str) -> anyhow::Result<Session> {
    loop {
        match try_create_session(token_path).await {
            Ok(session) => return Ok(session),
            Err(e) if e.to_string().contains("Bad credentials") => {
                warn!("Bad credentials — removing stale token and re-authenticating");
                std::fs::remove_file(token_path)
                    .context("Could not remove invalid token file")?;
                // loop retries
            }
            Err(e) => return Err(e),
        }
    }
}

async fn try_create_session(token_path: &str) -> anyhow::Result<Session> {
    let cache = make_cache()?;
    let (credentials, token) = resolve_credentials(token_path, &cache).await?;
    let session = connect(credentials, cache).await?;
    spawn_background_refresh(token_path.to_string(), token);
    Ok(session)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn token_expiring_in(secs: u64) -> TokenData {
        TokenData {
            access_token: "acc".to_string(),
            refresh_token: "ref".to_string(),
            expires_at: now_secs() + secs,
        }
    }

    // ── is_expiring_soon ──────────────────────────────────────────────────────

    #[test]
    fn should_report_expired_when_token_is_in_the_past() {
        // given: a token that expired an hour ago
        let token = token_expiring_in(0).expires_at.saturating_sub(3600);

        // when/then
        assert!(is_expiring_soon(token));
    }

    #[test]
    fn should_report_expiring_when_token_is_within_threshold() {
        // given: a token expiring in 4 minutes (below 10 minute threshold)
        let token = token_expiring_in(240);

        // when/then
        assert!(is_expiring_soon(token.expires_at));
    }

    #[test]
    fn should_not_report_expiring_when_token_has_ample_time_remaining() {
        // given: a token expiring in 1 hour
        let token = token_expiring_in(3600);

        // when/then
        assert!(!is_expiring_soon(token.expires_at));
    }

    // ── read_token / save_token ───────────────────────────────────────────────

    #[test]
    fn should_return_none_when_token_file_does_not_exist() {
        // given: a path to a non-existent file
        // when/then
        assert!(read_token("/tmp/no_such_token_file_xyz.json").is_none());
    }

    #[test]
    fn should_round_trip_token_data_through_disk() {
        // given: a token written to a temp file
        let file = NamedTempFile::new().unwrap();
        let token = TokenData {
            access_token: "my_acc".to_string(),
            refresh_token: "my_ref".to_string(),
            expires_at: 9999999999,
        };
        save_token(file.path().to_str().unwrap(), &token).unwrap();

        // when: the token is read back
        let loaded = read_token(file.path().to_str().unwrap()).unwrap();

        // then: all fields match
        assert_eq!(loaded.access_token, token.access_token);
        assert_eq!(loaded.refresh_token, token.refresh_token);
        assert_eq!(loaded.expires_at, token.expires_at);
    }

    #[test]
    fn should_return_none_for_malformed_token_file() {
        // given: a file with invalid JSON
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"not valid json { ]").unwrap();

        // when/then
        assert!(read_token(file.path().to_str().unwrap()).is_none());
    }
}
