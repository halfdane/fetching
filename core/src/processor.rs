//! Core business logic for processing Spotify URIs.
//!
//! Functions for downloading, caching, and playing Spotify content
//! including tracks, albums, and playlists.

use tracing::{error, info};
use uuid::Uuid;

use crate::cache::{cache_album, cache_playlist, get_track_with_ogg_format, process_track_cache};
use crate::config::Config;
use crate::implementations::LibrespotTrackFetcher;
use crate::metadata::build_track_path;

/// Process a single Spotify URI (track, album, or playlist) with a given task_id
pub async fn process_url(
    session: &librespot_core::session::Session,
    task_id: Uuid,
    url: &str,
    config: &Config,
) -> anyhow::Result<()> {
    let spotify_uri = match crate::input::parse_spotify_uri(url) {
        Ok(uri) => uri,
        Err(e) => {
            error!("❌ Failed to parse URI: {}", e);
            anyhow::bail!("Failed to parse URI: {}", e);
        }
    };
    process_single_uri(session, &spotify_uri, config, task_id).await
}

pub async fn handle_track(
    session: &librespot_core::session::Session,
    spotify_uri: &librespot_core::SpotifyUri,
    config: &Config,
    task_id: Uuid,
) -> anyhow::Result<()> {
    info!("Caching single track...");
    let track_fetcher = LibrespotTrackFetcher { session };
    let (track_provider, file_id) = get_track_with_ogg_format(&track_fetcher, spotify_uri).await?;

    let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;
    let music_dir_str = music_dir.to_str().ok_or_else(|| {
        anyhow::anyhow!(crate::error::DownloadError::InvalidUtf8Path(
            music_dir.clone()
        ))
    })?;
    let output_path = build_track_path(&*track_provider, music_dir_str).await?;

    let track_fetcher = LibrespotTrackFetcher { session };
    let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
    let image_downloader = crate::implementations::LibrespotImageDownloader { session };

    process_track_cache(
        &track_fetcher,
        &audio_downloader,
        &image_downloader,
        &*track_provider,
        spotify_uri,
        &output_path,
        &file_id,
        task_id,
        1,
        1,
    )
        .await?;
    Ok(())
}

async fn handle_album(
    session: &librespot_core::session::Session,
    spotify_uri: &librespot_core::SpotifyUri,
    config: &Config,
    task_id: Uuid,
) -> anyhow::Result<()> {

    let album_fetcher = crate::implementations::LibrespotAlbumFetcher { session };
    let track_fetcher = crate::implementations::LibrespotTrackFetcher { session };
    let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
    let image_downloader = crate::implementations::LibrespotImageDownloader { session };
    cache_album(
        &album_fetcher,
        &track_fetcher,
        &audio_downloader,
        &image_downloader,
        spotify_uri,
        config,
        task_id,
    )
        .await?;
    Ok(())
}

async fn handle_playlist(
    session: &librespot_core::session::Session,
    spotify_uri: &librespot_core::SpotifyUri,
    config: &Config,
    task_id: Uuid,
) -> anyhow::Result<()> {
    let playlist_fetcher = crate::implementations::LibrespotPlaylistFetcher { session };
    let track_fetcher = crate::implementations::LibrespotTrackFetcher { session };
    let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
    let image_downloader = crate::implementations::LibrespotImageDownloader { session };
    cache_playlist(
        &playlist_fetcher,
        &track_fetcher,
        &audio_downloader,
        &image_downloader,
        spotify_uri,
        config,
        task_id,
    )
        .await?;

    Ok(())
}

// Internal: process a single Spotify URI with a given task_id
async fn process_single_uri(
    session: &librespot_core::session::Session,
    spotify_uri: &librespot_core::SpotifyUri,
    config: &Config,
    task_id: Uuid,
) -> anyhow::Result<()> {
    match spotify_uri {
        librespot_core::SpotifyUri::Track { .. } => {
            handle_track(session, spotify_uri, config, task_id).await?;
        }
        librespot_core::SpotifyUri::Album { .. } => {
            handle_album(session, spotify_uri, config, task_id).await?;
        }
        librespot_core::SpotifyUri::Playlist { .. } => {
            handle_playlist(session, spotify_uri, config, task_id).await?;
        }
        _ => {
            anyhow::bail!(
                "Unsupported URI type. Only track, album, and playlist URIs are supported."
            );
        }
    }

    Ok(())
}
