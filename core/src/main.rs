use std::{collections::HashSet, path::PathBuf, sync::Arc};

use clap::Parser;
use fetching_core_lib::{
    audio_librespot::LibrespotAudioDownloader,
    librespot_impl::{
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
use tokio::sync::broadcast;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct Args {
    uri: String,
    /// Directory to store downloaded tracks under (default: /tmp/fetching-out)
    #[arg(short, long, default_value = "/tmp/fetching-out")]
    output_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let credentials_file = "/home/user/halfdane/.spotify_access_token";

    let session = Arc::new(create_session(&credentials_file).await?);

    // Build two independent fetchers – collection fetcher owns its track fetcher,
    // workers get their own handle via WorkerApis.
    let collection_fetcher = LibrespotCollectionMetadataFetcher::new(
        session.clone(),
        LibrespotTrackMetadataFetcher { session: session.clone() },
    );
    let cover_fetcher = LibrespotCoverFetcher::new(&session).await?;

    // Fetch collection metadata (blocking librespot call, still on main task here)
    let collection = Arc::new(collection_fetcher.fetch_by_uri(&args.uri)?);
    println!(
        "Collection: {} ({} tracks)",
        collection.title, collection.total_tracks
    );

    // Build the queue
    let apis = WorkerApis {
        collection_metadata: Arc::new(LibrespotCollectionMetadataFetcher::new(
            session.clone(),
            LibrespotTrackMetadataFetcher { session: session.clone() },
        )),
        track_metadata: Arc::new(LibrespotTrackMetadataFetcher { session: session.clone() }),
        cover: Arc::new(cover_fetcher),
        audio: Arc::new(LibrespotAudioDownloader::new(session.clone())),
    };

    let queue = Arc::new(TokioQueue::new(apis, DownloadRunner::new(args.output_dir.clone())));
    let mut progress_rx = queue.subscribe_progress();
    queue.start();

    // Enqueue all tracks from the collection
    let task_ids: HashSet<_> = queue.add_collection(Arc::clone(&collection)).into_iter().collect();
    let total = task_ids.len();
    println!("Queued {} track(s)", total);

    // Wait until every task is Done or Failed
    let mut finished = 0usize;
    loop {
        match progress_rx.recv().await {
            Ok(update) if task_ids.contains(&update.task_id) => {
                match &update.status {
                    TaskStatus::Done => {
                        finished += 1;
                        println!("[{}] done ({}/{})", update.task_id, finished, total);
                    }
                    TaskStatus::Failed { reason } => {
                        finished += 1;
                        eprintln!("[{}] failed: {} ({}/{})", update.task_id, reason, finished, total);
                    }
                    _ => {}
                }
                if finished == total {
                    break;
                }
            }
            Ok(_) => {}   // not one of our task_ids
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }

    println!("All done.");
    Ok(())
}
