use clap::{Parser, Subcommand};
use fetching_core::create_session;
use fetching_core::init_progress_tx;
use fetching_core::{config, SharedQueue};
use server_lib::server::{app, AppState};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "fetching", version, about = "Spotify Player CLI", author, disable_version_flag = true)]
struct Cli {
    /// Print build info (version + commit hash) and exit
    #[arg(long, help = "Print build info (version + commit hash) and exit")]
    hash: bool,
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
    let cli = Cli::parse();
    if cli.hash {
        let version = env!("CARGO_PKG_VERSION");
        let git_hash = env!("GIT_HASH");
        println!("fetching v{} (commit {})", version, git_hash);
        return Ok(());
    }
    match cli.command {
        Commands::Batch {
            urls,
            credentials_file} => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .init();

            let (raw_session, _refresher, _refresh_handle) =
                create_session(&credentials_file).await?;
            let session = Arc::new(raw_session);
            let config = config::Config::from_env();
            let (progress_tx, _) = tokio::sync::broadcast::channel(100);
            let (shared_queue, mut progress_rx) = SharedQueue::new(session, config, progress_tx.clone());

            let progress_handle = tokio::spawn(async move {
                loop {
                    match progress_rx.recv().await {
                        Ok(update) => {
                            let current_status = update.status.clone();
                            let current_user_id = update.user_visible_identifier.clone().unwrap_or_default();
                            println!("{} {}", current_status, current_user_id);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            continue;
                        }
                    }
                }
                println!("Queue complete!");
            });

            tracing::info!("About to add_tasks with {} URLs", urls.len());
            shared_queue.add_tasks(urls).await;
            tracing::info!(
                "add_tasks done, queue len: {}",
                { shared_queue.tasks.read().await }.len()
            );

            // Use a short idle timeout for batch mode (e.g., 500ms)
            let worker = shared_queue.clone().run_worker(Duration::from_millis(500));
            worker.await.expect("Worker panicked");

            // Drop all senders to close the channel and allow the progress reporter to exit
            drop(shared_queue);
            drop(progress_tx);
            let _ = progress_handle.await;
        }

        Commands::Server {
            port,
            credentials_file,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .init();

            let (raw_session, _refresher, _refresh_handle) =
                create_session(&credentials_file).await?;
            let session = Arc::new(raw_session);
            let config = config::Config::from_env();
            let _ = init_progress_tx(100);
            let tx = fetching_core::PROGRESS_TX.get().unwrap().clone();
            let (shared_queue, _) = SharedQueue::new(session, config, tx);
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
