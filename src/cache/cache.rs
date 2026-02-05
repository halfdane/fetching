//! High-level caching operations for albums and playlists.
//!
//! This module provides the main entry points for caching Spotify content,
//! coordinating between the various subsystems (track processing, image handling, etc.).

use std::collections::HashSet;
use std::path::PathBuf;
use librespot_core::SpotifyUri;
use tokio::time::{sleep, Duration};
use tracing::info;

use crate::cache::helpers::{get_artist_name_from_vec, format_track_display};
use crate::cache::images::save_cover_art;
use crate::cache::processors::{get_track_with_ogg_format, process_track_cache};
use crate::metadata::{build_track_path, sanitize};
use crate::m3u::{write_m3u_playlist, M3uEntry};

pub const TRACK_DELAY_MS: u64 = 200;

/// Cache all tracks in a collection and collect M3U entries
async fn cache_tracks_with_entries<'a, I>(
    track_fetcher: &dyn crate::traits::TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn crate::traits::ImageDownloader,
    tracks: I,
    total_tracks: usize,
    base_dir: &str,
    track_prefix: Option<fn(usize) -> String>,
    collect_album_covers: bool,
) -> anyhow::Result<(Vec<M3uEntry>, Vec<Vec<u8>>, Vec<PathBuf>)>
where
    I: Iterator<Item = &'a SpotifyUri>,
{
    let mut m3u_entries = Vec::new();
    let mut unique_album_covers: Vec<Vec<u8>> = Vec::new();
    let mut seen_album_ids: HashSet<String> = HashSet::new();
    let mut cached_paths = Vec::new();

    for (index, track_uri) in tracks.enumerate() {
        tracing::debug!("Processing track URI: {:?}", track_uri);

        // Get track with OGG format, trying alternatives if needed
        let (track_provider, file_id) = match get_track_with_ogg_format(track_fetcher, track_uri).await {
            Ok(result) => result,
            Err(e) => {
                let track_display = format_track_display(index + 1, total_tracks, "<unknown>");
                println!("{} ❌", track_display);
                tracing::error!("Failed to get track with OGG format: {}", e);

                // Continue to next track
                if index < total_tracks - 1 {
                    sleep(Duration::from_millis(TRACK_DELAY_MS)).await;
                }
                continue;
            }
        };

        let track_display = format_track_display(index + 1, total_tracks, &track_provider.name().await);
        print!("{}", track_display);
        std::io::Write::flush(&mut std::io::stdout())?;

        let prefix = track_prefix.map(|f| f(index + 1));
        let output_path = build_track_path(&*track_provider, base_dir, prefix).await?;

        match process_track_cache(track_fetcher, audio_downloader, image_downloader, &*track_provider, track_uri, &output_path, &file_id).await {
            Ok(()) => {
                // Collect album cover for collage if needed
                if collect_album_covers {
                    if let Err(e) = crate::cache::images::collect_album_cover(
                        image_downloader,
                        &*track_provider,
                        &mut unique_album_covers,
                        &mut seen_album_ids,
                    )
                    .await
                    {
                        tracing::warn!("Failed to collect album cover: {}", e);
                    }
                }

                // Add to M3U entries and cached paths
                m3u_entries.push(crate::m3u::build_m3u_entry(&*track_provider, output_path.clone()).await);
                cached_paths.push(output_path);
            }
            Err(_e) => {
                // Error already printed by process_track_cache, just add newline
                println!();
            }
        }

        // Small delay between tracks to avoid overwhelming the API
        if index < total_tracks - 1 {
            sleep(Duration::from_millis(TRACK_DELAY_MS)).await;
        }
    }

    Ok((m3u_entries, unique_album_covers, cached_paths))
}

/// Write M3U playlist and print summary
fn finalize_playlist(
    m3u_path: &std::path::Path,
    m3u_entries: &[M3uEntry],
    spotify_url: Option<&str>,
    total_tracks: usize,
) -> anyhow::Result<()> {
    if !m3u_entries.is_empty() {
        write_m3u_playlist(m3u_path, m3u_entries, spotify_url)?;
        info!("\nPlaylist file created: {}", m3u_path.display());
    }
    info!(
        "Cached {} of {} tracks",
        m3u_entries.len(),
        total_tracks
    );
    Ok(())
}

/// Cache a collection of tracks and create an M3U playlist.
///
/// Generic function that handles album or playlist streaming. Manages:
/// - Iterating through tracks with progress indicators
/// - Optional album cover collection for collages (playlists only)
/// - M3U playlist file generation with relative paths
/// - Cover art saving (single image or 2x2 collage)
///
/// # Errors
/// Returns error if:
/// - Music directory is not accessible
/// - Track streaming or caching fails (network, disk full, etc.)
/// - M3U file cannot be written
#[allow(clippy::too_many_arguments)]
pub async fn cache_track_collection<'a, I>(
    track_fetcher: &dyn crate::traits::TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn crate::traits::ImageDownloader,
    tracks: I,
    total_tracks: usize,
    base_dir: &str,
    track_prefix: Option<fn(usize) -> String>,
    _collection_name: &str,
    m3u_path: PathBuf,
    spotify_url: Option<String>,
    cover_art_bytes: Option<Vec<u8>>,
    collect_album_covers: bool,
) -> anyhow::Result<Vec<PathBuf>>
where
    I: Iterator<Item = &'a SpotifyUri>,
{
    // Cache all tracks and collect M3U entries
    let (m3u_entries, unique_album_covers, cached_paths) = cache_tracks_with_entries(
        track_fetcher,
        audio_downloader,
        image_downloader,
        tracks,
        total_tracks,
        base_dir,
        track_prefix,
        collect_album_covers,
    )
    .await?;

    // Save cover art (provided or collage)
    if let Err(e) = save_cover_art(&m3u_path, cover_art_bytes, &unique_album_covers) {
        tracing::warn!("Failed to save cover art: {}", e);
    }

    // Generate M3U playlist file and print summary
    finalize_playlist(
        &m3u_path,
        &m3u_entries,
        spotify_url.as_deref(),
        total_tracks,
    )?;

    Ok(cached_paths)
}

/// Stream and cache all tracks from an album.
///
/// # Errors
///
/// Returns error if:
/// - Album metadata cannot be retrieved
/// - Track streaming or caching operations fail
/// - M3U playlist file cannot be created
pub async fn cache_album(
    album_fetcher: &dyn crate::traits::AlbumFetcher,
    track_fetcher: &dyn crate::traits::TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn crate::traits::ImageDownloader,
    album_uri: &SpotifyUri,
    config: &crate::config::Config,
) -> anyhow::Result<Vec<PathBuf>> {
    info!("Fetching album metadata...");
    let album = album_fetcher.fetch_album(album_uri).await?;
    let album_name = album.album_name().await;
    let artists = album.album_artists().await;
    let artists_str = artists.join(", ");

    info!("Album: {} by {}", album_name, artists_str);

    let track_uris = album.album_track_uris().await;
    let total_tracks = track_uris.len();
    info!("Found {} tracks in album", total_tracks);

    let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;

    // Determine M3U file path - save in album directory
    // We'll construct it based on the first artist and album name
    let artist_name = sanitize(&get_artist_name_from_vec(&artists));
    let album_dir = music_dir.join(&artist_name).join(sanitize(&album_name));
    std::fs::create_dir_all(&album_dir)?;
    let m3u_path = album_dir.join(format!("{}.m3u8", sanitize(&album_name)));

    // Fetch album cover art
    let cover_file_ids = album.album_cover_file_ids().await;
    let cover_art = if let Some(cover_id) = cover_file_ids.first() {
        match image_downloader.download_cover(cover_id).await {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                tracing::warn!("Failed to fetch album cover art: {}", e);
                None
            }
        }
    } else {
        None
    };

    let spotify_url = Some(album_uri.to_string());

    let music_dir_str = music_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!(crate::error::DownloadError::InvalidUtf8Path(music_dir.clone())))?;
    cache_track_collection(
        track_fetcher,
        audio_downloader,
        image_downloader,
        track_uris.iter(),
        total_tracks,
        music_dir_str,
        None,
        "Album",
        m3u_path,
        spotify_url,
        cover_art,
        false, // Don't collect album covers for albums (we have the main cover)
    )
    .await
}

/// Prepare playlist directory and M3U file path
fn prepare_playlist_paths(
    music_dir: &std::path::Path,
    playlist_name: &str,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let playlists_dir = music_dir.join("Playlists");
    let playlist_dir = playlists_dir.join(sanitize(playlist_name));
    std::fs::create_dir_all(&playlist_dir)?;
    let m3u_path = playlist_dir.join(format!("{}.m3u8", sanitize(playlist_name)));
    Ok((playlist_dir, m3u_path))
}

/// Stream and cache all tracks from a playlist and create an M3U file
///
/// # Errors
///
/// Returns error if:
/// - Playlist metadata cannot be retrieved
/// - Track streaming or caching operations fail
/// - M3U playlist file cannot be created
pub async fn cache_playlist(
    playlist_fetcher: &dyn crate::traits::PlaylistFetcher,
    track_fetcher: &dyn crate::traits::TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn crate::traits::ImageDownloader,
    playlist_uri: &SpotifyUri,
    config: &crate::config::Config,
) -> anyhow::Result<Vec<PathBuf>> {
    info!("Fetching playlist metadata...");
    let playlist = playlist_fetcher.fetch_playlist(playlist_uri).await?;
    let playlist_name = playlist.playlist_name().await;
    info!("Playlist: {}", playlist_name);

    let track_uris = playlist.playlist_tracks().await;
    let total_tracks = track_uris.len();
    info!("Found {} tracks in playlist", total_tracks);

    let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;

    // Prepare directory structure and M3U path
    let (_playlist_dir, m3u_path) = prepare_playlist_paths(&music_dir, &playlist_name)?;

    // Get playlist cover art
    let cover_art = playlist.playlist_cover_art_bytes().await;

    let spotify_url = Some(playlist_uri.to_string());
    let music_dir_str = music_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!(crate::error::DownloadError::InvalidUtf8Path(music_dir.clone())))?;

    cache_track_collection(
        track_fetcher,
        audio_downloader,
        image_downloader,
        track_uris.iter(),
        total_tracks,
        music_dir_str,
        None,
        "Playlist",
        m3u_path,
        spotify_url,
        cover_art,
        true, // Collect album covers for playlist collage
    )
    .await
}