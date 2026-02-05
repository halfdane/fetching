use std::env;

use tracing::info;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::prelude::*;

mod auth;
mod cache;
mod cli;
mod config;
mod error;
mod implementations;
mod input;
mod m3u;
mod metadata;
mod mocks;
mod playback;
mod processor;
mod stream;
mod traits;

use cli::{validate_args, print_usage_and_exit, InputSource};
use config::Config;
use input::read_uris_from_file;
use processor::process_uris;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber with INFO level by default
    // Can be overridden with RUST_LOG environment variable
    let indicatif_layer = IndicatifLayer::new();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .with_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                ),
        )
        .with(indicatif_layer)
        .init();

    // Load configuration from environment variables
    let config = Config::from_env();

    let args: Vec<String> = env::args().collect();

    let (input_source, no_play) = match validate_args(&args) {
        Ok(result) => result,
        Err(_) => print_usage_and_exit(&args),
    };

    let token_path = ".spotify_access_token";

    // Create session with automatic background token refresh
    let (session, _refresher, _refresh_handle) = processor::create_session(token_path).await?;

    match input_source {
        InputSource::SingleUri(uri_arg) => {
            let uris = vec![uri_arg];
            process_uris(&session, &uris, &config, no_play).await?;
        }
        InputSource::File(path) => {
            let uris = read_uris_from_file(&path)?;
            info!("Loaded {} URIs from file", uris.len());
            process_uris(&session, &uris, &config, no_play).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_get_music_dir() {
        let config = Config::default();
        let music_dir = config.get_music_dir().expect("Failed to get music dir");
        assert!(music_dir.to_string_lossy().ends_with("Music"));
    }
}
