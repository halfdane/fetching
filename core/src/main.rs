use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use fetching_core_lib::{auth::session::create_session, container::dispatch_container, librespot_fetcher::LibrespotFetcher, metadata::SpotifyMetadata};


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

    let fetcher = LibrespotFetcher::new(&session).await?;
    
    let container = dispatch_container(&args.uri, &fetcher)?;
    
    // Print metadata (same as before)
    println!("Container: {} ({:?} tracks)", container.title, container.total_tracks);
    for track in &container.tracks {
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
