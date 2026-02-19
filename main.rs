use clap::{Parser, Subcommand};
use fetching_core::create_session;
use fetching_core::{config, SharedQueue};
use server_lib::server::{app, AppState};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

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
        #[arg(
            short = 'c',
            default_value = "credentials.json",
            long = "credentials-file",
            help = "Path to the credentials file"
        )]
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
        #[arg(
            short = 'c',
            default_value = "credentials.json",
            long = "credentials-file",
            help = "Path to the credentials file"
        )]
        credentials_file: String,
        /// Port to listen on
        #[arg(
            short,
            long,
            default_value_t = 8080,
            help = "Server port (overrides config)"
        )]
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

    let cli = Cli::parse();
    match cli.command {
        Commands::Batch {
            urls,
            credentials_file,
            queue,
        } => {
            let (raw_session, _refresher, _refresh_handle) =
                create_session(&credentials_file).await?;
            let session = Arc::new(raw_session);
            let config = config::Config::from_env();
            let (shared_queue, mut progress_rx) = SharedQueue::new(session, config, 100);

            tracing::info!("About to add_tasks with {} URLs", urls.len());
            shared_queue.add_tasks(urls).await;
            tracing::info!(
                "add_tasks done, queue len: {}",
                { shared_queue.tasks.read().await }.len()
            );

            if queue {
                let worker = shared_queue.run_worker(Duration::from_secs(10));

                let progress_handle = tokio::spawn(async move {
                    while let Ok(update) = progress_rx.recv().await {
                        println!("Task {}: {} {}/{}",
                                 update.task_id, update.status, update.current, update.total);
                    }
                    println!("Queue complete!");
                });

                tokio::try_join!(worker, progress_handle)?;
            } else {
                // Single: drain all sequentially and wait for completion before exiting
                while !shared_queue.is_empty().await {
                    let _ = shared_queue.process_next().await;
                }
            }
        }

        Commands::Server {
            port,
            credentials_file,
        } => {
            let (raw_session, _refresher, _refresh_handle) =
                create_session(&credentials_file).await?;
            let session = Arc::new(raw_session);
            let config = config::Config::from_env();
            let (shared_queue, mut progress_rx) = SharedQueue::new(session, config, 100);

            let queue_worker = shared_queue.clone();
            let worker = queue_worker.run_worker(Duration::MAX);

            let app_state = Arc::new(AppState {
                queue: shared_queue.clone(), // Now safe
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
            }
        }
    }

    Ok(())
}
