use std::{path::PathBuf, sync::Arc};

use clap::{Parser};
use fetching_core_lib::{auth::session::create_session, container::Track, librespot_fetcher::{LibrespotCollectionMetadataFetcher, LibrespotTrackMetadataFetcher}, spotify_api::{SpotifyTrackMetadata, fetch_collection}};

// Updated main.rs CLI
#[derive(Parser)]
struct Args {
    uri: String,
    #[arg(short, long)]
    covers_dir: Option<PathBuf>,
    #[arg(short, long)]
    fetch_covers: bool,
    #[arg(long)]
    username: Option<String>,
    #[arg(long)]
    password: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let credentials_file = "/home/user/halfdane/.spotify_access_token";
    let (raw_session, _refresher, _refresh_handle) =
        create_session(&credentials_file).await?;
    let session = Arc::new(raw_session);
    let track_fetcher = LibrespotTrackMetadataFetcher::new(&session).await?; 
    let collection_fetcher = LibrespotCollectionMetadataFetcher::new(&session, &track_fetcher).await?;
    
    let container = fetch_collection(&args.uri, &collection_fetcher)?;
    let tracks2: Vec<Track> = container.track_uris
        .iter()
        .map(|uri| track_fetcher.fetch_by_uri(uri).map(|(track, _)| track))
        .collect::<Result<Vec<_>, _>>()?;
    // Print metadata (same as before)
    println!("Container: {} ({:?} tracks)", container.title, container.total_tracks);
    for track in tracks2 {
        println!("  Track: {} ({}s)", track.title, track.duration_ms / 1000);
    }
    println!(">> {:#?}", &container);
        
    // if args.fetch_covers && args.covers_dir.is_some() {
    //     let cache = CoverCache::new(args.covers_dir.unwrap())?;
    //     cache.warm_from_container(&container).await?;
    //     println!("Covers saved!");
    // }
    
    Ok(())
}
