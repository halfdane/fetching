use tokio::sync::mpsc;
use tokio::sync::broadcast;

use spotify_player::cli::{validate_args, print_usage_and_exit, InputSource};
use spotify_player::input::read_uris_from_file;
use spotify_player::{spawn_task_processor, queue_uri_tasks, Task};
use server_lib::server::setup_and_run_server;

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

    let token_path = ".spotify_access_token";
    let (session, _refresher, _refresh_handle) = spotify_player::create_session(token_path).await?;
    let (progress_tx, progress_rx) = broadcast::channel(100);
    let (task_tx, task_rx) = mpsc::channel::<Task>(100);

    let processor_handle = spawn_task_processor(&session, task_rx, progress_tx.clone());

    server_lib::server::setup_and_run_server(task_tx, progress_tx).await?;

    Ok(())
}

