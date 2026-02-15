use std::env;

use tracing::info;
use tokio::sync::mpsc;

use spotify_player::create_session;
use spotify_player::cli::{validate_args, print_usage_and_exit, InputSource};
use spotify_player::config::Config;
use spotify_player::input::read_uris_from_file;
use spotify_player::process_uris;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber with INFO level by default
    // Can be overridden with RUST_LOG environment variable
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = env::args().collect();
    let input_source = match validate_args(&args) {
        Ok(result) => result,
        Err(_) => print_usage_and_exit(&args),
    };

    let (tx, _rx) = mpsc::channel(100);

    let mut any_error = false;
    match input_source {
        InputSource::SingleUri(uri_arg) => {
            let uris = vec![uri_arg];
            if let Err(e) = process_uris(&uris, tx.clone()).await {
                eprintln!("Error: {e}");
                any_error = true;
            }
        }
        InputSource::File(path) => {
            match read_uris_from_file(&path) {
                Ok(uris) => {
                    info!("Loaded {} URIs from file", uris.len());
                    if let Err(e) = process_uris(&uris, tx.clone()).await {
                        eprintln!("Error: {e}");
                        any_error = true;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read URIs from file: {e}");
                    any_error = true;
                }
            }
        }
    }
    if any_error {
        std::process::exit(1);
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
