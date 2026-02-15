//! Core business logic for processing Spotify URIs.
//!
//! Functions for downloading, caching, and playing Spotify content
//! including tracks, albums, and playlists.

use anyhow::Context;
use tracing::{error, info};
use uuid::Uuid;
use tokio::sync::mpsc;

use crate::auth::{create_session_with_auto_refresh, TokenRefresher};
use crate::cache::{cache_album, cache_playlist, process_track_cache, get_track_with_ogg_format};
use crate::config::Config;
use crate::metadata::build_track_path;
use crate::implementations::LibrespotTrackFetcher;

/// Process a single Spotify URI (track, album, or playlist)
async fn process_single_uri(
    session: &librespot_core::session::Session,
    spotify_uri: &librespot_core::SpotifyUri,
    config: &Config,
    tx: mpsc::Sender<crate::ProgressUpdate>,
    uri_arg: &str,
) -> anyhow::Result<()> {
    match spotify_uri {
        librespot_core::SpotifyUri::Track { .. } => {
            let task_id = Uuid::new_v4();
            tx.send(crate::ProgressUpdate {
                task_id,
                scope: crate::ProgressScope::Track,
                status: "Fetching track".to_string(),
                current: 0,
                total: 1,
                item: uri_arg.to_string(),
            }).await?;
            info!("Caching single track...");
            let track_fetcher = LibrespotTrackFetcher { session };
            let (track_provider, file_id) = get_track_with_ogg_format(&track_fetcher, spotify_uri).await?;

            let track_display = format!("Track: {}", track_provider.name().await);
            print!("{}", track_display);
            std::io::Write::flush(&mut std::io::stdout())?;

            tx.send(crate::ProgressUpdate {
                task_id,
                scope: crate::ProgressScope::Track,
                status: "Downloading".to_string(),
                current: 1,
                total: 1,
                item: track_display.clone(),
            }).await?;

            let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;
            let music_dir_str = music_dir.to_str().ok_or_else(|| {
                anyhow::anyhow!(crate::error::DownloadError::InvalidUtf8Path(music_dir.clone()))
            })?;
            let output_path = build_track_path(&*track_provider, music_dir_str, None).await?;

            let track_fetcher = LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = crate::implementations::LibrespotImageDownloader { session };

            process_track_cache(&track_fetcher, &audio_downloader, &image_downloader, &*track_provider, spotify_uri, &output_path, &file_id).await?;

            tx.send(crate::ProgressUpdate {
                task_id,
                scope: crate::ProgressScope::Track,
                status: "Completed".to_string(),
                current: 1,
                total: 1,
                item: track_display,
            }).await?;
        }
        librespot_core::SpotifyUri::Album { .. } => {
            let task_id = Uuid::new_v4();
            tx.send(crate::ProgressUpdate {
                task_id,
                scope: crate::ProgressScope::Album,
                status: "Fetching album".to_string(),
                current: 0,
                total: 1,
                item: uri_arg.to_string(),
            }).await?;
            let album_fetcher = crate::implementations::LibrespotAlbumFetcher { session };
            let track_fetcher = crate::implementations::LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = crate::implementations::LibrespotImageDownloader { session };
            let cached_paths = cache_album(&album_fetcher, &track_fetcher, &audio_downloader, &image_downloader, spotify_uri, config, &tx, task_id).await?;
            tx.send(crate::ProgressUpdate {
                task_id,
                scope: crate::ProgressScope::Album,
                status: "Completed".to_string(),
                current: 1,
                total: 1,
                item: uri_arg.to_string(),
            }).await?;
        }
        librespot_core::SpotifyUri::Playlist { .. } => {
            let task_id = Uuid::new_v4();
            tx.send(crate::ProgressUpdate {
                task_id,
                scope: crate::ProgressScope::Playlist,
                status: "Fetching playlist".to_string(),
                current: 0,
                total: 1,
                item: uri_arg.to_string(),
            }).await?;
            let playlist_fetcher = crate::implementations::LibrespotPlaylistFetcher { session };
            let track_fetcher = crate::implementations::LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = crate::implementations::LibrespotImageDownloader { session };
            let cached_paths = cache_playlist(&playlist_fetcher, &track_fetcher, &audio_downloader, &image_downloader, spotify_uri, config, &tx, task_id).await?;
            tx.send(crate::ProgressUpdate {
                task_id,
                scope: crate::ProgressScope::Playlist,
                status: "Completed".to_string(),
                current: 1,
                total: 1,
                item: uri_arg.to_string(),
            }).await?;
        }
        _ => {
            anyhow::bail!(
                "Unsupported URI type. Only track, album, and playlist URIs are supported."
            );
        }
    }

    Ok(())
}

/// Process multiple Spotify URIs with error handling and summary
pub async fn process_uris(
    session: &librespot_core::session::Session,
    uris: &[String],
    config: &Config,
    tx: mpsc::Sender<crate::ProgressUpdate>,
) -> anyhow::Result<()> {
    let mut successful = 0;
    let mut failed: Vec<(String, String)> = Vec::new();

    let show_progress = uris.len() > 1;

    for (index, uri_arg) in uris.iter().enumerate() {
        if show_progress {
            let current = index + 1;
            let total = uris.len();
            info!("Processing {} of {}: {}", current, total, uri_arg);
        }

        let spotify_uri = match crate::input::parse_spotify_uri(uri_arg) {
            Ok(uri) => uri,
            Err(e) => {
                error!("❌ Failed to parse URI: {}", e);
                failed.push((uri_arg.clone(), e.to_string()));
                continue;
            }
        };

        match process_single_uri(session, &spotify_uri, config, tx.clone(), uri_arg).await {
            Ok(_) => successful += 1,
            Err(e) => {
                error!("❌ Failed to process: {}", e);
                failed.push((uri_arg.clone(), e.to_string()));
            }
        }
    }

    // Show summary for multiple URIs or if there were any failures
    if uris.len() > 1 || !failed.is_empty() {
        info!("");
        info!("Summary:");
        info!("  Total: {}", uris.len());
        info!("  ✅ Successful: {}", successful);
        info!("  ❌ Failed: {}", failed.len());

        if !failed.is_empty() {
            info!("");
            info!("Failed URIs:");
            for (uri, error) in &failed {
                info!("  - {} ({})", uri, error);
            }
        }
    }

    // Return error only if ALL failed
    if successful == 0 && !failed.is_empty() {
        anyhow::bail!("All URIs failed to process");
    }

    Ok(())
}
