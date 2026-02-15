use std::env;

use tracing::info;
use tokio::sync::mpsc;
use librespot_core::Session;
use uuid::Uuid;

use spotify_player::create_session;
use spotify_player::cli::{validate_args, print_usage_and_exit, InputSource};
use spotify_player::config::Config;
use spotify_player::input::read_uris_from_file;
use spotify_player::process_single_url;

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


    // Run CLI mode
    run_cli(args).await?;

    Ok(())
}

async fn run_cli(args: Vec<String>) -> anyhow::Result<()> {
    let input_source = match validate_args(&args) {
        Ok(result) => result,
        Err(_) => print_usage_and_exit(&args),
    };

    // Create session once
    let token_path = ".spotify_access_token";
    let (session, _refresher, _refresh_handle) = spotify_player::auth::session::create_session(token_path).await?;

    let (tx, _rx) = mpsc::channel(100);

    let mut any_error = false;
    match input_source {
        InputSource::SingleUri(uri_arg) => {
            let task_id = uuid::Uuid::new_v4();
            if let Err(e) = process_single_url(&session, task_id, uri_arg, tx.clone()).await {
                eprintln!("Error: {e}");
                any_error = true;
            }
        }
        InputSource::File(path) => {
            match read_uris_from_file(&path) {
                Ok(uris) => {
                    info!("Loaded {} URIs from file", uris.len());
                    for uri in uris {
                        let task_id = uuid::Uuid::new_v4();
                        if let Err(e) = process_single_url(&session, task_id, uri, tx.clone()).await {
                            eprintln!("Error: {e}");
                            any_error = true;
                        }
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

