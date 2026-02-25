use std::{collections::HashSet, path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use fetching_core_lib::{
    audio_librespot::LibrespotAudioDownloader,
    librespot_impl::{
        cached_cover_fetcher::CachedCoverProvider,
        collection_metadata::LibrespotCollectionMetadataFetcher,
        cover_fetcher::LibrespotCoverFetcher,
        session::create_session,
        track_metadata::LibrespotTrackMetadataFetcher,
    },
    queue::{TaskStatus, WorkerApis},
    queue_tokio::TokioQueue,
    runner::DownloadRunner,
    spotify_api::SpotifyCollectionMetadata,
};
use server_lib::server::{app, AppState};
use tokio::sync::broadcast;

#[derive(Parser)]
#[command(name = "fetching", version, about = "Spotify downloader", author)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Download Spotify URLs (batch mode)")]
    Batch {
        #[arg(
            short = 'c',
            default_value = "credentials.json",
            long = "credentials-file",
            help = "Path to the credentials file"
        )]
        credentials_file: String,
        #[arg(
            short = 'o',
            long = "output-dir",
            default_value = ".",
            help = "Directory to save downloaded tracks under"
        )]
        output_dir: PathBuf,
        /// One or more Spotify track/album/playlist URIs or URLs
        #[arg(required = true, help = "One or more Spotify URIs")]
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
        #[arg(
            short = 'o',
            long = "output-dir",
            default_value = ".",
            help = "Directory to save downloaded tracks under"
        )]
        output_dir: PathBuf,
        #[arg(
            short,
            long,
            default_value_t = 8080,
            help = "Server port"
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
        Commands::Batch { urls, credentials_file, output_dir } => {
            let session = Arc::new(create_session(&credentials_file).await?);

            // Fetcher used only for pre-resolving URIs on the main task
            let collection_fetcher = LibrespotCollectionMetadataFetcher::new(
                session.clone(),
                LibrespotTrackMetadataFetcher { session: session.clone() },
            );
            let cover_fetcher = LibrespotCoverFetcher::new(&session).await?;

            // Build the queue with its own independent fetcher handles
            let apis = WorkerApis {
                collection_metadata: Arc::new(LibrespotCollectionMetadataFetcher::new(
                    session.clone(),
                    LibrespotTrackMetadataFetcher { session: session.clone() },
                )),
                track_metadata: Arc::new(LibrespotTrackMetadataFetcher { session: session.clone() }),
                cover: Arc::new(cover_fetcher),
                audio: Arc::new(LibrespotAudioDownloader::new(session.clone())),
            };

            let queue = Arc::new(TokioQueue::new(apis, DownloadRunner::new(output_dir)));
            let mut progress_rx = queue.subscribe_progress();
            queue.start();

            // Resolve each URI and enqueue; collect all task IDs
            let mut all_task_ids: HashSet<_> = HashSet::new();
            for url in &urls {
                match collection_fetcher.fetch_by_uri(url) {
                    Ok(collection) => {
                        let collection = Arc::new(collection);
                        tracing::info!(
                            "Resolved '{}': {} tracks",
                            collection.title,
                            collection.total_tracks
                        );
                        let ids = queue.add_collection(Arc::clone(&collection));
                        tracing::info!("Queued {} track(s), queue depth: {}", ids.len(), queue.len());
                        all_task_ids.extend(ids);
                    }
                    Err(e) => {
                        tracing::error!("Failed to resolve '{url}': {e}");
                    }
                }
            }

            let total = all_task_ids.len();
            if total == 0 {
                tracing::warn!("Nothing to download.");
                return Ok(());
            }

            tracing::info!("Waiting for {total} track(s) to finish...");

            // Wait until every enqueued task reaches Done or Failed
            let mut finished = 0usize;
            loop {
                match progress_rx.recv().await {
                    Ok(update) if all_task_ids.contains(&update.task_id) => {
                        match &update.status {
                            TaskStatus::Done => {
                                finished += 1;
                                tracing::info!("[{}] done ({}/{})", update.task_id, finished, total);
                            }
                            TaskStatus::Failed { reason } => {
                                finished += 1;
                                tracing::warn!("[{}] failed: {} ({}/{})", update.task_id, reason, finished, total);
                            }
                            _ => {}
                        }
                        if finished == total {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            tracing::info!("All done.");
        }

        Commands::Server { port, credentials_file, output_dir } => {
            let session = Arc::new(create_session(&credentials_file).await?);

            // CachedCoverProvider wraps Arc<Cache> internally — cloning is cheap
            // and shares the same cache between the worker and the HTTP handler.
            let raw_cover = LibrespotCoverFetcher::new(&session).await?;
            let cover = CachedCoverProvider::new(Arc::new(raw_cover));

            let apis = WorkerApis {
                collection_metadata: Arc::new(LibrespotCollectionMetadataFetcher::new(
                    session.clone(),
                    LibrespotTrackMetadataFetcher { session: session.clone() },
                )),
                track_metadata: Arc::new(LibrespotTrackMetadataFetcher { session: session.clone() }),
                cover: Arc::new(cover.clone()),
                audio: Arc::new(LibrespotAudioDownloader::new(session.clone())),
            };

            let queue = Arc::new(TokioQueue::new(apis, DownloadRunner::new(output_dir)));
            queue.start();

            let app_state = Arc::new(AppState {
                queue: Arc::clone(&queue),
                collection_metadata: Arc::new(LibrespotCollectionMetadataFetcher::new(
                    session.clone(),
                    LibrespotTrackMetadataFetcher { session: session.clone() },
                )),
                cover: Arc::new(cover),
            });

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
            tracing::info!("Server listening on http://0.0.0.0:{port}");
            axum::serve(listener, app(app_state)).await?;
        }
    }

    Ok(())
}
