use std::env;

use tracing::info;
use tokio::sync::mpsc;

use spotify_player::cli::{validate_args, print_usage_and_exit, InputSource};
use spotify_player::input::read_uris_from_file;
use spotify_player::{spawn_task_processor, queue_uri_tasks, Task};

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

    let token_path = ".spotify_access_token";
    let (session, _refresher, _refresh_handle) = spotify_player::create_session(token_path).await?;
    let (tx, _rx) = mpsc::channel(100);
    let (task_tx, task_rx) = mpsc::channel::<Task>(100);

    let processor_handle = spawn_task_processor(&session, task_rx, tx);

    let uris = match input_source {
        InputSource::SingleUri(uri) => vec![uri],
        InputSource::File(path) => {
            let uris = read_uris_from_file(&path)?;
            info!("Loaded {} URIs from file", uris.len());
            uris
        }
    };

    let task_tx_clone = task_tx.clone();
    queue_uri_tasks(uris, task_tx_clone).await?;

    drop(task_tx);  // Close channel

    let any_error = processor_handle.await.unwrap_or(true);
    if any_error {
        std::process::exit(1);
    }
    Ok(())
}

