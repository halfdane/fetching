use std::env;
use std::sync::Arc;

use tracing::info;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use spotify_player::auth::TokenRefresher;
use spotify_player::cli::{validate_args, print_usage_and_exit, InputSource};
use spotify_player::input::read_uris_from_file;
use spotify_player::process_single_url;

#[derive(Debug)]
struct Task {
    task_id: Uuid,
    uri: String,
}

async fn create_session() -> anyhow::Result<(librespot_core::session::Session, Arc<TokenRefresher>, JoinHandle<()>)> {
    let token_path = ".spotify_access_token";
    spotify_player::auth::session::create_session(token_path).await
}

fn spawn_processor(
    session: &librespot_core::session::Session,
    task_rx: mpsc::Receiver<Task>,
    tx: mpsc::Sender<spotify_player::ProgressUpdate>,
) -> JoinHandle<bool> {
    let session_clone = session.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut task_rx: mpsc::Receiver<Task> = task_rx;
        let mut any_error = false;
        while let Some(task) = task_rx.recv().await {
            let uri = task.uri.clone();
            if let Err(e) = process_single_url(&session_clone, task.task_id, uri, tx_clone.clone()).await {
                eprintln!("Error processing {}: {e}", task.uri);
                any_error = true;
            }
        }
        any_error
    })
}

async fn queue_tasks(
    input_source: InputSource,
    task_tx: mpsc::Sender<Task>,
) -> anyhow::Result<()> {
    match input_source {
        InputSource::SingleUri(uri_arg) => {
            let task_id = Uuid::new_v4();
            let task = Task { task_id, uri: uri_arg };
            task_tx.send(task).await.map_err(|_| anyhow::anyhow!("Queue send failed"))?;
        }
        InputSource::File(path) => {
            let uris = read_uris_from_file(&path)?;
            info!("Loaded {} URIs from file", uris.len());
            for uri in uris {
                let task_id = Uuid::new_v4();
                let task = Task { task_id, uri };
                task_tx.send(task).await.map_err(|_| anyhow::anyhow!("Queue send failed"))?;
            }
        }
    }
    Ok(())
}

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

    let (session, _refresher, _refresh_handle) = create_session().await?;
    let (tx, _rx) = mpsc::channel(100);
    let (task_tx, task_rx) = mpsc::channel::<Task>(100);

    let processor_handle = spawn_processor(&session, task_rx, tx);

    let task_tx_clone = task_tx.clone();
    queue_tasks(input_source, task_tx_clone).await?;

    drop(task_tx);  // Close channel

    let any_error = processor_handle.await.unwrap_or(true);
    if any_error {
        std::process::exit(1);
    }
    Ok(())
}

