use std::sync::Arc;
use std::time::Duration;
use clap::{Parser, Subcommand};
use tokio::signal;
use tokio::sync::{mpsc, broadcast};
use uuid::Uuid;
use fetching_core::{SharedQueue, Task, processor, config};
use server_lib::server::{app, setup_and_run_server, AppState};

use fetching_core::create_session;


#[derive(Parser)]
#[command(name = "fetching", version, about = "Spotify Player CLI", author)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Process Spotify URLs (batch mode)")]
    Batch {
        #[arg(short = 'c', default_value = "credentials.json", long = "credentials-file", help = "Path to the credentials file")] 
        credentials_file: String,
        /// Spotify track/album/playlist URLs (or @file.txt)
        #[arg(required = true, help = "One or more Spotify URLs")]
        urls: Vec<String>,
        /// Optional flag to use queue processing (default: false)
        #[arg(short, long, help = "Use queue processing (default: false)")]
        queue: bool,
    },
    #[command(about = "Start web server + download queue")]
    Server {
        #[arg(short = 'c', default_value = "credentials.json", long = "credentials-file", help = "Path to the credentials file")] 
        credentials_file: String,
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080, help = "Server port (overrides config)")]
        port: u16,
    },
}

#[tokio::main(flavor = "multi_thread", worker_threads = 3)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    run_cli().await?;

    Ok(())
}

async fn run_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Batch { urls, credentials_file, queue } => {
            let (raw_session, _refresher, _refresh_handle) = create_session(&credentials_file).await?;
            let (progress_tx, _progress_rx) = broadcast::channel(100);
            let session = Arc::new(raw_session);  // CRITICAL: Arc<Session>
            let config = config::Config::from_env();
            let shared_queue = Arc::new(SharedQueue::new(session, config, progress_tx.clone()));

            tracing::info!("About to add_tasks with {} URLs", urls.len());  // CLI arg count
            shared_queue.add_tasks(urls).await;
            tracing::info!("add_tasks done, queue len: {}", {shared_queue.tasks.read().await}.len());

            if queue {
                let worker = shared_queue.run_worker(Duration::from_secs(5));  // Exit 5s idle
                tokio::select! {
                    _ = worker => { tracing::info!("Worker completed"); }
                }
            } else {
                // Single: drain all sequentially (or .next() for true single)
                while !shared_queue.is_empty().await {
                    let _ = shared_queue.process_next().await;
                }
            }
        }

        Commands::Server { port, credentials_file } => {
            let (raw_session, _refresher, _refresh_handle) = create_session(&credentials_file).await?;
            let session = Arc::new(raw_session);
            let (progress_tx, _progress_rx) = broadcast::channel(100);
            let config = config::Config::from_env();
            let shared_queue = Arc::new(SharedQueue::new(session, config, progress_tx.clone()));

            let queue_worker = shared_queue.clone();  // Clone FIRST for worker
            let worker = queue_worker.run_worker(Duration::from_secs(3600));

            let app_state = Arc::new(AppState {
                queue: shared_queue.clone(),  // Now safe
                progress_tx: progress_tx.clone()
            });

            let listener = tokio::net::TcpListener::bind(&format!("0.0.0.0:{}", port)).await?;
            tracing::info!("Server listening on http://0.0.0.0:{}", port);



            tokio::select! {
                res = axum::serve(listener, app(app_state)) => {
                    if let Err(e) = res {
                        tracing::warn!("Server error: {e}");
                    }
                }
                _ = worker => { tracing::warn!("Worker exited"); }
            };
        }
    }

    Ok(())
}

