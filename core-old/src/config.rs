//! Application configuration with environment variable support.
//!
//! Provides sensible defaults for all settings while allowing override
//! via environment variables. Key settings include OAuth credentials,
//! music directory location, and streaming behavior (retries, delays).

use std::path::PathBuf;

/// Application configuration with sensible defaults
#[derive(Debug, Clone)]
pub struct Config {
    /// OAuth client ID for Spotify authentication
    pub oauth_client_id: String,

    /// OAuth redirect URI
    pub redirect_uri: String,

    /// Base music directory (defaults to ~/Music)
    pub music_dir: Option<PathBuf>,

    /// Delay between track streaming operations in milliseconds
    pub track_delay_ms: u64,

    /// Maximum number of retry attempts for streaming
    pub max_retries: u32,

    /// Base delay between retries in milliseconds
    pub retry_delay_ms: u64,

    /// Network operation timeout in seconds
    pub network_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oauth_client_id: "65b708073fc0480ea92a077233ca87bd".to_string(),
            redirect_uri: "http://127.0.0.1:8898/login".to_string(),
            music_dir: None, // Will use ~/Music
            track_delay_ms: 200,
            max_retries: 5,
            retry_delay_ms: 1000,
            network_timeout_secs: 30,
        }
    }
}

impl Config {
    /// Create config from environment variables, falling back to defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(client_id) = std::env::var("SPOTIFY_CLIENT_ID") {
            config.oauth_client_id = client_id;
        }

        if let Ok(redirect) = std::env::var("SPOTIFY_REDIRECT_URI") {
            config.redirect_uri = redirect;
        }

        if let Ok(music_dir) = std::env::var("MUSIC_DIR") {
            config.music_dir = Some(PathBuf::from(music_dir));
        }

        if let Ok(delay) = std::env::var("TRACK_DELAY_MS") {
            if let Ok(val) = delay.parse() {
                config.track_delay_ms = val;
            }
        }

        if let Ok(retries) = std::env::var("MAX_RETRIES") {
            if let Ok(val) = retries.parse() {
                config.max_retries = val;
            }
        }

        if let Ok(delay) = std::env::var("RETRY_DELAY_MS") {
            if let Ok(val) = delay.parse() {
                config.retry_delay_ms = val;
            }
        }

        if let Ok(timeout) = std::env::var("NETWORK_TIMEOUT_SECS") {
            if let Ok(val) = timeout.parse() {
                config.network_timeout_secs = val;
            }
        }

        config
    }

    /// Get the music directory, using config override or ~/Music default
    pub fn get_music_dir(&self) -> crate::error::Result<PathBuf> {
        if let Some(ref dir) = self.music_dir {
            return Ok(dir.clone());
        }

        let mut music_dir = PathBuf::from("/data");
        music_dir.push("Music");
        Ok(music_dir)
    }
}
