//! Track, album, and playlist streaming with local caching.
//!
//! This module handles batch streaming and caching of Spotify content:
//! - Single track caching with metadata tagging
//! - Album streaming with cover art
//! - Playlist streaming with cover collages from multiple albums

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use librespot_core::SpotifyUri;
use librespot_metadata::audio::AudioFileFormat;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::stream::stream_and_cache_track;
use crate::error::DownloadError;
use crate::traits::{TrackMetadataProvider, ImageDownloader, TrackFetcher, AlbumFetcher, PlaylistFetcher};
use crate::implementations::OwnedLibrespotTrackProvider;
use crate::m3u::{write_m3u_playlist, M3uEntry};
use crate::metadata::{build_track_path, sanitize, write_ogg_tags, TrackMetadata};

/// Extract artist name from a list of artist names, returning "Unknown Artist" if empty
pub fn get_artist_name_from_vec(artists: &[String]) -> String {
    if !artists.is_empty() {
        artists[0].clone()
    } else {
        "Unknown Artist".to_string()
    }
}

pub const TRACK_DELAY_MS: u64 = 200;

/// Collect unique album covers for collage creation (up to 4 unique albums)
pub async fn collect_album_cover(
    image_downloader: &dyn crate::traits::ImageDownloader,
    metadata: &dyn TrackMetadataProvider,
    unique_covers: &mut Vec<Vec<u8>>,
    seen_album_ids: &mut HashSet<String>,
) -> anyhow::Result<()> {
    if unique_covers.len() >= 4 {
        return Ok(());
    }

    let album_id = metadata.album_id().await;
    if seen_album_ids.contains(&album_id) {
        return Ok(());
    }

    seen_album_ids.insert(album_id);
    if let Some(file_id) = metadata.get_album_cover_file_id(0).await {
        match image_downloader.download_cover(&file_id).await {
            Ok(bytes) => unique_covers.push(bytes),
            Err(e) => warn!("Failed to fetch album cover for collage: {}", e),
        }
    }

    Ok(())
}

/// Save cover art to a file (either provided bytes or collage from album covers)
pub fn save_cover_art(
    m3u_path: &Path,
    cover_bytes: Option<Vec<u8>>,
    unique_album_covers: &[Vec<u8>],
) -> anyhow::Result<()> {
    let parent_dir = m3u_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("M3U path has no parent directory"))?;
    let cover_path = parent_dir.join("cover.jpg");

    if let Some(bytes) = cover_bytes {
        println!("Saving cover art to: {}", cover_path.display());
        std::fs::write(&cover_path, bytes)?;
        println!("Cover art saved successfully");
    } else if !unique_album_covers.is_empty() {
        info!(
            "Creating cover collage from {} album covers",
            unique_album_covers.len()
        );
        let collage_bytes = create_cover_collage(unique_album_covers)?;
        std::fs::write(&cover_path, collage_bytes)?;
        info!("Cover collage saved successfully");
    } else {
        debug!("No cover art bytes to save");
    }

    Ok(())
}

/// Build an M3U entry from track metadata
pub async fn build_m3u_entry(metadata: &dyn TrackMetadataProvider, output_path: PathBuf) -> M3uEntry {
    let artist_names = metadata.artist_names().await;
    let artist = artist_names.first().cloned().unwrap_or_else(|| "Unknown Artist".to_string());

    M3uEntry {
        duration: (metadata.duration_ms().await / 1000) as i32,
        artist,
        title: metadata.name().await,
        file_path: output_path,
    }
}

/// Create a 2x2 collage from up to 4 album cover images
pub fn create_cover_collage(cover_images: &[Vec<u8>]) -> anyhow::Result<Vec<u8>> {
    use image::{DynamicImage, ImageFormat, RgbImage};

    if cover_images.is_empty() {
        return Err(anyhow::anyhow!("No cover images to create collage"));
    }

    // Load and resize all images to 300x300
    let tile_size = 300u32;
    let mut tiles: Vec<DynamicImage> = Vec::new();

    for (i, img_bytes) in cover_images.iter().take(4).enumerate() {
        match image::load_from_memory(img_bytes) {
            Ok(img) => {
                let resized =
                    img.resize_exact(tile_size, tile_size, image::imageops::FilterType::Lanczos3);
                tiles.push(resized);
            }
            Err(e) => {
                warn!("Failed to load cover image {}: {}", i, e);
            }
        }
    }

    if tiles.is_empty() {
        return Err(anyhow::anyhow!("Failed to load any cover images"));
    }

    let grid_size = if tiles.len() == 1 { 1 } else { 2 };
    let canvas_size = tile_size * grid_size;
    let mut canvas = RgbImage::new(canvas_size, canvas_size);

    for (idx, tile) in tiles.iter().enumerate() {
        let row = (idx / 2) as u32;
        let col = (idx % 2) as u32;
        let x = col * tile_size;
        let y = row * tile_size;

        image::imageops::replace(&mut canvas, &tile.to_rgb8(), x as i64, y as i64);
    }

    // Encode to JPEG
    let mut output = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(canvas).write_to(&mut output, ImageFormat::Jpeg)?;
    Ok(output.into_inner())
}

/// Build a temporary file path from an output path
pub fn build_temp_path(output_path: &Path) -> PathBuf {
    let mut temp_path = output_path.to_path_buf();
    // Keep .ogg extension so lofty can detect the format
    temp_path.set_extension("tmp.ogg");
    temp_path
}

/// Generate track display string for console output
pub fn format_track_display(index: usize, total: usize, track_name: &str) -> String {
    format!("Track {} of {}: {}", index, total, track_name)
}

/// Get the best quality OGG Vorbis file ID and format for a track
/// Returns (FileId, AudioFileFormat) if found
async fn select_best_ogg_file<T: TrackMetadataProvider>(
    track: &T,
) -> Option<(librespot_core::file_id::FileId, AudioFileFormat)> {
    // Try formats in order of quality (highest to lowest)
    if let Some(file_id) = track.get_file_id(&AudioFileFormat::OGG_VORBIS_320).await {
        return Some((file_id, AudioFileFormat::OGG_VORBIS_320));
    }
    if let Some(file_id) = track.get_file_id(&AudioFileFormat::OGG_VORBIS_160).await {
        return Some((file_id, AudioFileFormat::OGG_VORBIS_160));
    }
    if let Some(file_id) = track.get_file_id(&AudioFileFormat::OGG_VORBIS_96).await {
        return Some((file_id, AudioFileFormat::OGG_VORBIS_96));
    }
    None
}

/// Get a track with OGG Vorbis format, selecting the highest quality from original and alternatives
/// Returns (Track, FileId) where the track has the best available OGG format
pub async fn get_track_with_ogg_format(
    track_fetcher: &dyn TrackFetcher,
    uri: &SpotifyUri,
) -> anyhow::Result<(Box<dyn TrackMetadataProvider>, librespot_core::file_id::FileId)> {
    let track = track_fetcher.fetch_track(uri).await?;
    let provider = OwnedLibrespotTrackProvider { track: track.clone() };
    
    // Collect all candidates: original track + all alternatives with their OGG format
    let mut candidates: Vec<(Box<dyn TrackMetadataProvider>, librespot_core::file_id::FileId, AudioFileFormat, String)> = Vec::new();
    
    // Check original track
    if let Some((file_id, format)) = select_best_ogg_file(&provider).await {
        // Early termination: if original has highest quality (320), no need to check alternatives
        if format == AudioFileFormat::OGG_VORBIS_320 {
            debug!("Track '{}' has OGG_VORBIS_320 in original, skipping alternatives", provider.track.name);
            return Ok((Box::new(OwnedLibrespotTrackProvider { track: track.clone() }), file_id));
        }
        candidates.push((Box::new(OwnedLibrespotTrackProvider { track: track.clone() }), file_id, format, "original".to_string()));
    }
    
    // Check all alternatives if original doesn't exist or doesn't have best quality
    let alternative_uris = provider.alternative_uris().await;
    if candidates.is_empty() || !alternative_uris.is_empty() {
        debug!("Track '{}' checking {} alternatives for better quality", provider.track.name, alternative_uris.len());
        
        for (i, alt_uri_str) in alternative_uris.iter().enumerate() {
            let alt_uri = SpotifyUri::from_uri(alt_uri_str)?;
            match track_fetcher.fetch_track(&alt_uri).await {
                Ok(alt_track) => {
                    let alt_provider = OwnedLibrespotTrackProvider { track: alt_track.clone() };
                    if let Some((file_id, format)) = select_best_ogg_file(&alt_provider).await {
                        candidates.push((Box::new(OwnedLibrespotTrackProvider { track: alt_track.clone() }), file_id, format, format!("alternative {}", i + 1)));
                    }
                }
                Err(e) => {
                    debug!("  Alternative {} failed to fetch: {}", i + 1, e);
                }
            }
        }
    }
    
    // Select the best quality from all candidates
    if candidates.is_empty() {
        anyhow::bail!("Track '{}' not available in OGG Vorbis format (tried {} alternatives)", track.name, alternative_uris.len())
    }
    
    // Sort by format quality (320 > 160 > 96)
    candidates.sort_by_key(|(_, _, format, _)| match format {
        AudioFileFormat::OGG_VORBIS_320 => 0,
        AudioFileFormat::OGG_VORBIS_160 => 1,
        AudioFileFormat::OGG_VORBIS_96 => 2,
        _ => 3,
    });
    
    let (best_track, file_id, format, source) = candidates.into_iter().next().unwrap();
    info!("Selected {:?} from {} for track '{}'", format, source, best_track.name().await);
    
    Ok((best_track, file_id))
}

async fn cache_track_cover_art(image_downloader: &dyn ImageDownloader, metadata: &dyn TrackMetadataProvider) -> Option<Vec<u8>> {
    if let Some(file_id) = metadata.get_album_cover_file_id(0).await {
        print!(" 🖼️");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        match image_downloader.download_cover(&file_id).await {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                // Print error inline without newline to keep on same line as track
                print!(" ❌[cover]");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                warn!("Failed to fetch cover art: {}", e);
                None
            }
        }
    } else {
        None
    }
}

/// Write metadata to temp file and handle cleanup on error
fn write_metadata_to_temp(temp_path: &Path, metadata: &TrackMetadata) -> anyhow::Result<()> {
    let temp_path_str = temp_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!(DownloadError::InvalidUtf8Path(temp_path.to_path_buf())))?;

    if let Err(e) = write_ogg_tags(temp_path_str, metadata) {
        if temp_path.exists() {
            if let Err(cleanup_err) = std::fs::remove_file(temp_path) {
                warn!(
                    "Failed to clean up temp file after metadata error: {}",
                    cleanup_err
                );
            }
        }
        print!(" ❌");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        error!("Failed to write metadata: {}", e);
        return Err(e);
    }
    Ok(())
}

/// Rename temp file to final location with cleanup on error
fn finalize_track_file(temp_path: &Path, output_path: &Path) -> anyhow::Result<()> {
    if let Err(e) = std::fs::rename(temp_path, output_path) {
        if temp_path.exists() {
            if let Err(cleanup_err) = std::fs::remove_file(temp_path) {
                warn!(
                    "Failed to clean up temp file after rename error: {}",
                    cleanup_err
                );
            }
        }
        print!(" ❌");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        error!("Failed to finalize file: {}", e);
        return Err(e.into());
    }
    Ok(())
}

/// Process a single track: check existence, cache, add metadata
pub async fn process_track_cache(
    _track_fetcher: &dyn TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn ImageDownloader,
    track_provider: &dyn TrackMetadataProvider,
    track_uri: &SpotifyUri,
    output_path: &Path,
    file_id: &librespot_core::file_id::FileId,
) -> anyhow::Result<()> {
    // Check if file already exists
    if output_path.exists() {
        println!(" ✅");
        std::io::Write::flush(&mut std::io::stdout())?;
        return Ok(());
    }

    print!(" 📥");
    std::io::Write::flush(&mut std::io::stdout())?;

    let temp_path = build_temp_path(output_path);

    // Clean up any existing temp file
    if temp_path.exists() {
        if let Err(e) = std::fs::remove_file(&temp_path) {
            warn!("Failed to remove existing temp file: {}", e);
        }
    }

    let temp_path_str = match temp_path.to_str() {
        Some(s) => s,
        None => {
            print!(" ❌");
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let err = anyhow::anyhow!(DownloadError::InvalidUtf8Path(temp_path.clone()));
            error!("Invalid UTF-8 path: {}", err);
            return Err(err);
        }
    };

    if let Err(e) = stream_and_cache_track(
        audio_downloader,
        file_id,
        track_uri,
        temp_path_str,
        None, // Use default retry delays in production
    )
    .await
    {
        if temp_path.exists() {
            if let Err(cleanup_err) = std::fs::remove_file(&temp_path) {
                warn!(
                    "Failed to clean up temp file after streaming error: {}",
                    cleanup_err
                );
            }
        }
        print!(" ❌");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        error!("Failed to stream track: {}", e);
        return Err(e);
    }

    // Fetch cover art
    let cover_art = cache_track_cover_art(image_downloader, track_provider).await;

    // Add metadata to the temp file
    let metadata = TrackMetadata::from_provider(track_provider, cover_art).await;
    if let Err(e) = write_metadata_to_temp(&temp_path, &metadata) {
        // Error already printed by write_metadata_to_temp
        return Err(e);
    }

    // Rename temp file to final output path
    if let Err(e) = finalize_track_file(&temp_path, output_path) {
        // Error already printed by finalize_track_file
        return Err(e);
    }

    println!(" ✅");
    Ok(())
}

/// Cache all tracks in a collection and collect M3U entries
async fn cache_tracks_with_entries<'a, I>(
    track_fetcher: &dyn TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn ImageDownloader,
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
        debug!("Processing track URI: {:?}", track_uri);
        
        // Get track with OGG format, trying alternatives if needed
        let (track_provider, file_id) = match get_track_with_ogg_format(track_fetcher, track_uri).await {
            Ok(result) => result,
            Err(e) => {
                let track_display = format_track_display(index + 1, total_tracks, "<unknown>");
                println!("{} ❌", track_display);
                error!("Failed to get track with OGG format: {}", e);
                
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
                    if let Err(e) = collect_album_cover(
                        image_downloader,
                        &*track_provider,
                        &mut unique_album_covers,
                        &mut seen_album_ids,
                    )
                    .await
                    {
                        warn!("Failed to collect album cover: {}", e);
                    }
                }

                // Add to M3U entries and cached paths
                m3u_entries.push(build_m3u_entry(&*track_provider, output_path.clone()).await);
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
    m3u_path: &Path,
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
    track_fetcher: &dyn TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn ImageDownloader,
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
        warn!("Failed to save cover art: {}", e);
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
    album_fetcher: &dyn AlbumFetcher,
    track_fetcher: &dyn TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn ImageDownloader,
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
                warn!("Failed to fetch album cover art: {}", e);
                None
            }
        }
    } else {
        None
    };

    let spotify_url = Some(album_uri.to_string());

    let music_dir_str = music_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!(DownloadError::InvalidUtf8Path(music_dir.clone())))?;
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
    music_dir: &Path,
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
/// Stream and cache all tracks from a playlist and create an M3U file
///
/// # Errors
///
/// Returns error if:
/// - Playlist metadata cannot be retrieved
/// - Track streaming or caching operations fail
/// - M3U playlist file cannot be created
pub async fn cache_playlist(
    playlist_fetcher: &dyn PlaylistFetcher,
    track_fetcher: &dyn TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn ImageDownloader,
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
        .ok_or_else(|| anyhow::anyhow!(DownloadError::InvalidUtf8Path(music_dir.clone())))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_temp_path() {
        let output = PathBuf::from("/tmp/music/artist/album/track.ogg");
        let temp = build_temp_path(&output);

        assert_eq!(
            temp.to_str().unwrap(),
            "/tmp/music/artist/album/track.tmp.ogg"
        );
        assert!(temp.to_string_lossy().ends_with(".tmp.ogg"));
    }

    #[test]
    fn test_build_temp_path_preserves_directory() {
        let output = PathBuf::from("/path/to/file.ogg");
        let temp = build_temp_path(&output);

        assert_eq!(temp.parent(), output.parent());
    }

    #[test]
    fn test_format_track_display() {
        let display = format_track_display(1, 10, "Test Track");
        assert_eq!(display, "Track 1 of 10: Test Track");
    }

    #[test]
    fn test_format_track_display_double_digits() {
        let display = format_track_display(42, 100, "Another Song");
        assert_eq!(display, "Track 42 of 100: Another Song");
    }

    #[test]
    fn test_track_delay_constant() {
        const { assert!(TRACK_DELAY_MS > 0) };
        const { assert!(TRACK_DELAY_MS < 10000) }; // Reasonable delay
    }

    // Tests for select_best_ogg_file
    
    
}
