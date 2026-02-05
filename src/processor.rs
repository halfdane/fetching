//! Core business logic for processing Spotify URIs.
//!
//! Functions for downloading, caching, and playing Spotify content
//! including tracks, albums, and playlists.

use anyhow::Context;
use tracing::{error, info};

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
    no_play: bool,
) -> anyhow::Result<()> {
    match spotify_uri {
        librespot_core::SpotifyUri::Track { .. } => {
            info!("Caching single track...");
            let track_fetcher = LibrespotTrackFetcher { session };
            let (track_provider, file_id) = get_track_with_ogg_format(&track_fetcher, spotify_uri).await?;

            let track_display = format!("Track: {}", track_provider.name().await);
            print!("{}", track_display);
            std::io::Write::flush(&mut std::io::stdout())?;

            let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;
            let music_dir_str = music_dir.to_str().ok_or_else(|| {
                anyhow::anyhow!(crate::error::DownloadError::InvalidUtf8Path(music_dir.clone()))
            })?;
            let output_path = build_track_path(&*track_provider, music_dir_str, None).await?;

            let track_fetcher = LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = crate::implementations::LibrespotImageDownloader { session };

            process_track_cache(&track_fetcher, &audio_downloader, &image_downloader, &*track_provider, spotify_uri, &output_path, &file_id).await?;

            if !no_play {
                info!("\nStarting playback...");
                crate::playback::play_audio_file(&output_path)?;
            }
        }
        librespot_core::SpotifyUri::Album { .. } => {
            let album_fetcher = crate::implementations::LibrespotAlbumFetcher { session };
            let track_fetcher = crate::implementations::LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = crate::implementations::LibrespotImageDownloader { session };
            let cached_paths = cache_album(&album_fetcher, &track_fetcher, &audio_downloader, &image_downloader, spotify_uri, config).await?;

            if !no_play && !cached_paths.is_empty() {
                info!("\nStarting album playback...");
                crate::playback::play_audio_files(&cached_paths)?;
            }
        }
        librespot_core::SpotifyUri::Playlist { .. } => {
            let playlist_fetcher = crate::implementations::LibrespotPlaylistFetcher { session };
            let track_fetcher = crate::implementations::LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = crate::implementations::LibrespotImageDownloader { session };
            let cached_paths = cache_playlist(&playlist_fetcher, &track_fetcher, &audio_downloader, &image_downloader, spotify_uri, config).await?;

            if !no_play && !cached_paths.is_empty() {
                info!("\nStarting playlist playback...");
                crate::playback::play_audio_files(&cached_paths)?;
            }
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
    no_play: bool,
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

        match process_single_uri(session, &spotify_uri, config, no_play).await {
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

/// Create a Spotify session with automatic token refresh
pub async fn create_session(token_path: &str) -> anyhow::Result<(librespot_core::session::Session, std::sync::Arc<TokenRefresher>, tokio::task::JoinHandle<()>)> {
    loop {
        match create_session_with_auto_refresh(token_path).await {
            Ok(result) => break Ok(result),
            Err(e) if e.to_string().contains("Bad credentials") => {
                std::fs::remove_file(token_path).context("Failed to remove invalid token file")?;
                // Retry will trigger new OAuth flow
            }
            Err(e) => return Err(e),
        }
    }
}