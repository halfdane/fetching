use clap::{Parser, Subcommand};
use fetching_core::create_session;
use fetching_core::{config, SharedQueue};
use server_lib::server::{app, AppState};
use std::sync::Arc;
use std::time::Duration;

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
    match cli.command {
        Commands::Batch {
            urls,
            credentials_file} => {
            let (raw_session, _refresher, _refresh_handle) =
                create_session(&credentials_file).await?;
            let session = Arc::new(raw_session);
            let config = config::Config::from_env();
            let (shared_queue, mut progress_rx) = SharedQueue::new(session, config, 100);

            tokio::spawn(async move {
                while let Ok(update) = progress_rx.recv().await {
                    let current_status = update.status;
                    let current_user_id = update.user_visible_identifier.clone().unwrap_or_default();
                    println!("{} {}",
                             current_status, current_user_id);
                }
                println!("Queue complete!");
            });

            tracing::info!("About to add_tasks with {} URLs", urls.len());
            shared_queue.add_tasks(urls).await;
            tracing::info!(
                "add_tasks done, queue len: {}",
                { shared_queue.tasks.read().await }.len()
            );

            while !shared_queue.is_empty().await {
                let _ = shared_queue.process_next().await;
            }
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
            let (shared_queue, _) = SharedQueue::new(session, config, 100);
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
