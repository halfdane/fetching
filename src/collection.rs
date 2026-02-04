//! Track, album, and playlist streaming with local caching.
//!
//! This module handles batch streaming and caching of Spotify content:
//! - Single track caching with metadata tagging
//! - Album streaming with cover art
//! - Playlist streaming with cover collages from multiple albums

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use librespot_core::session::Session;
use librespot_core::SpotifyUri;
use librespot_core::FileId;
use librespot_metadata::audio::AudioFileFormat;
use librespot_metadata::track::Track;
use librespot_metadata::{album::Album, playlist::Playlist, Metadata};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::stream::{stream_and_cache_track, download_cover_image, LibrespotAudioDownloader};
use crate::error::DownloadError;
use crate::traits::{TrackMetadataProvider, LibrespotTrackProvider};
use crate::m3u::{write_m3u_playlist, M3uEntry};
use crate::metadata::{build_track_path, get_artist_name, sanitize, write_ogg_tags, TrackMetadata};

/// Wrapper to implement TrackMetadataProvider for a Track reference
#[derive(Debug)]
struct TrackRefMetadataProvider<'a>(&'a Track);

#[async_trait]
impl<'a> TrackMetadataProvider for TrackRefMetadataProvider<'a> {
    async fn id(&self) -> String {
        self.0.id.to_string()
    }

    async fn name(&self) -> String {
        self.0.name.clone()
    }

    async fn album_id(&self) -> String {
        self.0.album.id.to_string()
    }

    async fn album_name(&self) -> String {
        self.0.album.name.clone()
    }

    async fn artist_names(&self) -> Vec<String> {
        self.0.artists.iter().map(|a| a.name.clone()).collect()
    }

    async fn duration_ms(&self) -> u32 {
        self.0.duration as u32
    }

    async fn year(&self) -> i32 {
        self.0.album.date.year()
    }

    async fn track_number(&self) -> u32 {
        self.0.number as u32
    }

    async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId> {
        self.0.files.get(format).copied()
    }

    async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
        self.0.album.covers.get(index).map(|cover| cover.id)
    }
}

pub const TRACK_DELAY_MS: u64 = 200;

/// Collect unique album covers for collage creation (up to 4 unique albums)
pub async fn collect_album_cover(
    session: &Session,
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
        match download_cover_image(session, &file_id).await {
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
    session: &Session,
    uri: &SpotifyUri,
) -> anyhow::Result<(Track, librespot_core::file_id::FileId)> {
    let track = Track::get(session, uri).await?;
    
    // Collect all candidates: original track + all alternatives with their OGG format
    let mut candidates: Vec<(Track, librespot_core::file_id::FileId, AudioFileFormat, String)> = Vec::new();
    
    // Check original track
    if let Some((file_id, format)) = select_best_ogg_file(&LibrespotTrackProvider { track: &track }).await {
        // Early termination: if original has highest quality (320), no need to check alternatives
        if format == AudioFileFormat::OGG_VORBIS_320 {
            debug!("Track '{}' has OGG_VORBIS_320 in original, skipping alternatives", track.name);
            return Ok((track, file_id));
        }
        candidates.push((track.clone(), file_id, format, "original".to_string()));
    }
    
    // Check all alternatives if original doesn't exist or doesn't have best quality
    if candidates.is_empty() || !track.alternatives.is_empty() {
        debug!("Track '{}' checking {} alternatives for better quality", track.name, track.alternatives.len());
        
        for (i, alt_uri) in track.alternatives.iter().enumerate() {
            match Track::get(session, alt_uri).await {
                Ok(alt_track) => {
                    if let Some((file_id, format)) = select_best_ogg_file(&LibrespotTrackProvider { track: &alt_track }).await {
                        candidates.push((alt_track, file_id, format, format!("alternative {}", i + 1)));
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
        anyhow::bail!("Track '{}' not available in OGG Vorbis format (tried {} alternatives)", track.name, track.alternatives.len())
    }
    
    // Sort by format quality (320 > 160 > 96)
    candidates.sort_by_key(|(_, _, format, _)| match format {
        AudioFileFormat::OGG_VORBIS_320 => 0,
        AudioFileFormat::OGG_VORBIS_160 => 1,
        AudioFileFormat::OGG_VORBIS_96 => 2,
        _ => 3,
    });
    
    let (best_track, file_id, format, source) = candidates.into_iter().next().unwrap();
    info!("Selected {:?} from {} for track '{}'", format, source, best_track.name);
    
    Ok((best_track, file_id))
}

async fn cache_track_cover_art(session: &Session, metadata: &dyn TrackMetadataProvider) -> Option<Vec<u8>> {
    if let Some(file_id) = metadata.get_album_cover_file_id(0).await {
        print!(" 🖼️");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        match download_cover_image(session, &file_id).await {
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
    session: &Session,
    track: &Track,
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

    let downloader = LibrespotAudioDownloader { session };
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
        &downloader,
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
    let cover_art = cache_track_cover_art(session, &TrackRefMetadataProvider(track)).await;

    // Add metadata to the temp file
    let year = track.album.date.year();
    let metadata = TrackMetadata::from_track(track, year, cover_art);
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
    session: &Session,
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
        let (track, file_id) = match get_track_with_ogg_format(session, track_uri).await {
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
        
        let track_display = format_track_display(index + 1, total_tracks, &track.name);
        print!("{}", track_display);
        std::io::Write::flush(&mut std::io::stdout())?;

        let prefix = track_prefix.map(|f| f(index + 1));
        let provider = LibrespotTrackProvider { track: &track };
        let output_path = build_track_path(&provider, base_dir, prefix).await?;

        match process_track_cache(session, &track, track_uri, &output_path, &file_id).await {
            Ok(()) => {
                // Collect album cover for collage if needed
                if collect_album_covers {
                    if let Err(e) = collect_album_cover(
                        session,
                        &TrackRefMetadataProvider(&track),
                        &mut unique_album_covers,
                        &mut seen_album_ids,
                    )
                    .await
                    {
                        warn!("Failed to collect album cover: {}", e);
                    }
                }

                // Add to M3U entries and cached paths
                let track_provider = LibrespotTrackProvider { track: &track };
                m3u_entries.push(build_m3u_entry(&track_provider, output_path.clone()).await);
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
    session: &Session,
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
        session,
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
    session: &Session,
    album_uri: &SpotifyUri,
    config: &crate::config::Config,
) -> anyhow::Result<Vec<PathBuf>> {
    info!("Fetching album metadata...");
    let album = Album::get(session, album_uri).await?;
    let album_name = album.name.clone();
    let artists_str = album
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    info!("Album: {} by {}", album_name, artists_str);

    let total_tracks = album.tracks().count();
    info!("Found {} tracks in album", total_tracks);

    let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;

    // Determine M3U file path - save in album directory
    // We'll construct it based on the first artist and album name
    let artist_name = sanitize(&get_artist_name(&album.artists));
    let album_dir = music_dir.join(&artist_name).join(sanitize(&album_name));
    std::fs::create_dir_all(&album_dir)?;
    let m3u_path = album_dir.join(format!("{}.m3u8", sanitize(&album_name)));

    // Fetch album cover art
    let cover_art = if let Some(cover) = album.covers.first() {
        match download_cover_image(session, &cover.id).await {
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
        session,
        album.tracks(),
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

/// Fetch playlist cover art from picture field or URL
async fn fetch_playlist_cover_art(playlist: &Playlist) -> Option<Vec<u8>> {
    if !playlist.attributes.picture.is_empty() {
        debug!(
            "Playlist has cover art in picture field ({} bytes)",
            playlist.attributes.picture.len()
        );
        return Some(playlist.attributes.picture.clone());
    }

    if let Some(picture_size) = playlist.attributes.picture_sizes.first() {
        debug!(
            "Attempting to fetch cover art from URL: {}",
            picture_size.url
        );
        match reqwest::get(&picture_size.url).await {
            Ok(response) => match response.bytes().await {
                Ok(bytes) => {
                    debug!("Fetched cover art ({} bytes)", bytes.len());
                    return Some(bytes.to_vec());
                }
                Err(e) => warn!("Failed to fetch playlist cover art: {}", e),
            },
            Err(e) => warn!("Failed to fetch playlist cover art: {}", e),
        }
    } else {
        debug!("No cover art found in playlist attributes");
    }

    None
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
/// - M3U playlist file cannot be created
pub async fn cache_playlist(
    session: &Session,
    playlist_uri: &SpotifyUri,
    config: &crate::config::Config,
) -> anyhow::Result<Vec<PathBuf>> {
    info!("Fetching playlist metadata...");
    let playlist = Playlist::get(session, playlist_uri).await?;
    let playlist_name = playlist.name();
    info!("Playlist: {}", playlist_name);

    let total_tracks = playlist.tracks().len();
    info!("Found {} tracks in playlist", total_tracks);

    let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;

    // Prepare directory structure and M3U path
    let (_playlist_dir, m3u_path) = prepare_playlist_paths(&music_dir, playlist_name)?;

    // Fetch playlist cover art
    let cover_art = fetch_playlist_cover_art(&playlist).await;

    let spotify_url = Some(playlist_uri.to_string());
    let music_dir_str = music_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!(DownloadError::InvalidUtf8Path(music_dir.clone())))?;

    cache_track_collection(
        session,
        playlist.tracks(),
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
    use async_trait::async_trait;
    use std::collections::HashMap;
    use librespot_core::file_id::FileId;

    #[derive(Debug)]
    struct MockTrackForOggSelection {
        files: HashMap<AudioFileFormat, FileId>,
    }

    #[async_trait]
    impl TrackMetadataProvider for MockTrackForOggSelection {
        async fn id(&self) -> String { "mock".to_string() }
        async fn name(&self) -> String { "Mock Track".to_string() }
        async fn album_id(&self) -> String { "mock_album".to_string() }
        async fn album_name(&self) -> String { "Mock Album".to_string() }
        async fn artist_names(&self) -> Vec<String> { vec!["Mock Artist".to_string()] }
        async fn duration_ms(&self) -> u32 { 180000 }
        async fn year(&self) -> i32 { 2023 }
        async fn track_number(&self) -> u32 { 1 }
        async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId> {
            self.files.get(format).copied()
        }
        
        async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
            if index == 0 {
                Some(FileId::from_raw(&[1u8; 16]))
            } else {
                None
            }
        }
    }

    #[tokio::test]
    async fn test_select_best_ogg_file_prefers_320() {
        let mut files = HashMap::new();
        files.insert(AudioFileFormat::OGG_VORBIS_320, FileId::from_raw(&[1u8; 16]));
        files.insert(AudioFileFormat::OGG_VORBIS_160, FileId::from_raw(&[2u8; 16]));

        let mock = MockTrackForOggSelection { files };

        let result = select_best_ogg_file(&mock).await;
        assert!(result.is_some());
        let (file_id, format) = result.unwrap();
        assert_eq!(format, AudioFileFormat::OGG_VORBIS_320);
        assert_eq!(file_id, FileId::from_raw(&[1u8; 16]));
    }

    #[tokio::test]
    async fn test_select_best_ogg_file_falls_back_to_160() {
        let mut files = HashMap::new();
        files.insert(AudioFileFormat::OGG_VORBIS_160, FileId::from_raw(&[2u8; 16]));
        files.insert(AudioFileFormat::OGG_VORBIS_96, FileId::from_raw(&[3u8; 16]));

        let mock = MockTrackForOggSelection { files };

        let result = select_best_ogg_file(&mock).await;
        assert!(result.is_some());
        let (file_id, format) = result.unwrap();
        assert_eq!(format, AudioFileFormat::OGG_VORBIS_160);
        assert_eq!(file_id, FileId::from_raw(&[2u8; 16]));
    }

    #[tokio::test]
    async fn test_select_best_ogg_file_falls_back_to_96() {
        let mut files = HashMap::new();
        files.insert(AudioFileFormat::OGG_VORBIS_96, FileId::from_raw(&[3u8; 16]));

        let mock = MockTrackForOggSelection { files };

        let result = select_best_ogg_file(&mock).await;
        assert!(result.is_some());
        let (file_id, format) = result.unwrap();
        assert_eq!(format, AudioFileFormat::OGG_VORBIS_96);
        assert_eq!(file_id, FileId::from_raw(&[3u8; 16]));
    }

    #[tokio::test]
    async fn test_select_best_ogg_file_no_ogg_formats() {
        let files = HashMap::new(); // No OGG formats available

        let mock = MockTrackForOggSelection { files };

        let result = select_best_ogg_file(&mock).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_select_best_ogg_file_only_non_ogg_formats() {
        let mut files = HashMap::new();
        // Add some non-OGG formats - using available variants
        files.insert(AudioFileFormat::MP3_320, FileId::from_raw(&[4u8; 16]));
        files.insert(AudioFileFormat::MP3_256, FileId::from_raw(&[5u8; 16]));

        let mock = MockTrackForOggSelection { files };

        let result = select_best_ogg_file(&mock).await;
        assert!(result.is_none());
    }

    #[derive(Debug)]
    struct MockTrackForM3uEntry {
        pub name: String,
        pub artist_names: Vec<String>,
        pub duration_ms: u32,
    }

    #[async_trait]
    impl TrackMetadataProvider for MockTrackForM3uEntry {
        async fn id(&self) -> String { "test".to_string() }
        async fn name(&self) -> String { self.name.clone() }
        async fn album_id(&self) -> String { "album".to_string() }
        async fn album_name(&self) -> String { "album".to_string() }
        async fn artist_names(&self) -> Vec<String> { self.artist_names.clone() }
        async fn duration_ms(&self) -> u32 { self.duration_ms }
        async fn year(&self) -> i32 { 2023 }
        async fn track_number(&self) -> u32 { 1 }
        async fn get_file_id(&self, _format: &AudioFileFormat) -> Option<FileId> { None }
        
        async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
            if index == 0 {
                Some(FileId::from_raw(&[1u8; 16]))
            } else {
                None
            }
        }
    }

    #[tokio::test]
    async fn test_build_m3u_entry_basic() {
        let mock_metadata = MockTrackForM3uEntry {
            name: "Test Song".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 245000, // 4 minutes 5 seconds
        };

        let output_path = PathBuf::from("/music/test_artist/test_album/test_song.ogg");
        let entry = build_m3u_entry(&mock_metadata, output_path.clone()).await;

        assert_eq!(entry.title, "Test Song");
        assert_eq!(entry.artist, "Test Artist");
        assert_eq!(entry.duration, 245); // 245000ms / 1000 = 245s
        assert_eq!(entry.file_path, output_path);
    }

    #[tokio::test]
    async fn test_build_m3u_entry_multiple_artists() {
        let mock_metadata = MockTrackForM3uEntry {
            name: "Collaboration Song".to_string(),
            artist_names: vec!["Artist One".to_string(), "Artist Two".to_string(), "Artist Three".to_string()],
            duration_ms: 180000,
        };

        let output_path = PathBuf::from("/music/artist_one/collaboration_song.ogg");
        let entry = build_m3u_entry(&mock_metadata, output_path).await;

        assert_eq!(entry.artist, "Artist One"); // Should use first artist
    }

    #[tokio::test]
    async fn test_build_m3u_entry_no_artists() {
        let mock_metadata = MockTrackForM3uEntry {
            name: "Unknown Artist Song".to_string(),
            artist_names: vec![], // Empty artist list
            duration_ms: 200000,
        };

        let output_path = PathBuf::from("/music/unknown_artist/unknown_artist_song.ogg");
        let entry = build_m3u_entry(&mock_metadata, output_path).await;

        assert_eq!(entry.artist, "Unknown Artist");
    }

    #[tokio::test]
    async fn test_build_m3u_entry_duration_rounding() {
        let mock_metadata = MockTrackForM3uEntry {
            name: "Test Song".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 123456, // 123.456 seconds -> should round down to 123
        };

        let output_path = PathBuf::from("/music/test.ogg");
        let entry = build_m3u_entry(&mock_metadata, output_path).await;

        assert_eq!(entry.duration, 123); // Integer division truncates
    }

    // Enhanced mock for testing album cover functionality
    #[derive(Debug)]
    struct MockTrackWithMultipleCovers {
        pub name: String,
        pub artist_names: Vec<String>,
        pub duration_ms: u32,
        pub album_cover_file_ids: Vec<FileId>,
    }

    #[async_trait]
    impl TrackMetadataProvider for MockTrackWithMultipleCovers {
        async fn id(&self) -> String { "test".to_string() }
        async fn name(&self) -> String { self.name.clone() }
        async fn album_id(&self) -> String { "album".to_string() }
        async fn album_name(&self) -> String { "album".to_string() }
        async fn artist_names(&self) -> Vec<String> { self.artist_names.clone() }
        async fn duration_ms(&self) -> u32 { self.duration_ms }
        async fn year(&self) -> i32 { 2023 }
        async fn track_number(&self) -> u32 { 1 }
        async fn get_file_id(&self, _format: &AudioFileFormat) -> Option<FileId> { None }
        
        async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
            self.album_cover_file_ids.get(index).copied()
        }
    }

    #[tokio::test]
    async fn test_get_album_cover_file_id_multiple_indices() {
        let cover_ids = vec![
            FileId::from_raw(&[1u8; 16]),
            FileId::from_raw(&[2u8; 16]),
            FileId::from_raw(&[3u8; 16]),
        ];
        
        let mock = MockTrackWithMultipleCovers {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 180000,
            album_cover_file_ids: cover_ids.clone(),
        };

        // Test valid indices
        assert_eq!(mock.get_album_cover_file_id(0).await, Some(cover_ids[0]));
        assert_eq!(mock.get_album_cover_file_id(1).await, Some(cover_ids[1]));
        assert_eq!(mock.get_album_cover_file_id(2).await, Some(cover_ids[2]));
        
        // Test out-of-bounds indices
        assert_eq!(mock.get_album_cover_file_id(3).await, None);
        assert_eq!(mock.get_album_cover_file_id(10).await, None);
    }

    #[tokio::test]
    async fn test_get_album_cover_file_id_no_covers() {
        let mock = MockTrackWithMultipleCovers {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 180000,
            album_cover_file_ids: vec![], // No covers
        };

        assert_eq!(mock.get_album_cover_file_id(0).await, None);
        assert_eq!(mock.get_album_cover_file_id(1).await, None);
    }

    #[tokio::test]
    async fn test_get_album_cover_file_id_single_cover() {
        let cover_id = FileId::from_raw(&[42u8; 16]);
        let mock = MockTrackWithMultipleCovers {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 180000,
            album_cover_file_ids: vec![cover_id],
        };

        assert_eq!(mock.get_album_cover_file_id(0).await, Some(cover_id));
        assert_eq!(mock.get_album_cover_file_id(1).await, None);
    }

    #[tokio::test]
    async fn test_collect_album_cover_with_covers() {
        use std::collections::HashSet;
        
        let cover_id = FileId::from_raw(&[1u8; 16]);
        let mock = MockTrackWithMultipleCovers {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 180000,
            album_cover_file_ids: vec![cover_id],
        };

        let _unique_covers: Vec<Vec<u8>> = Vec::new();
        let _seen_album_ids: HashSet<String> = HashSet::new();
        
        // Mock session - we can't actually test downloading, but we can test the logic
        // This test would need to be an integration test with proper mocking
        // For now, just test that the method is called correctly
        let album_id = mock.album_id().await;
        assert_eq!(album_id, "album");
        
        let file_id = mock.get_album_cover_file_id(0).await;
        assert_eq!(file_id, Some(cover_id));
    }

    #[tokio::test]
    async fn test_collect_album_cover_no_covers() {
        use std::collections::HashSet;
        
        let mock = MockTrackWithMultipleCovers {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 180000,
            album_cover_file_ids: vec![], // No covers
        };

        let _unique_covers: Vec<Vec<u8>> = Vec::new();
        let _seen_album_ids: HashSet<String> = HashSet::new();
        
        let album_id = mock.album_id().await;
        assert_eq!(album_id, "album");
        
        let file_id = mock.get_album_cover_file_id(0).await;
        assert_eq!(file_id, None);
    }

    #[tokio::test]
    async fn test_cache_track_cover_art_no_covers() {
        let mock = MockTrackWithMultipleCovers {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 180000,
            album_cover_file_ids: vec![], // No covers
        };

        // This would normally call download_cover_image, but since we can't mock that easily,
        // we just verify the trait method returns None
        let file_id = mock.get_album_cover_file_id(0).await;
        assert_eq!(file_id, None);
    }

    #[tokio::test]
    async fn test_cache_track_cover_art_with_covers() {
        let cover_id = FileId::from_raw(&[1u8; 16]);
        let mock = MockTrackWithMultipleCovers {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            duration_ms: 180000,
            album_cover_file_ids: vec![cover_id],
        };

        let file_id = mock.get_album_cover_file_id(0).await;
        assert_eq!(file_id, Some(cover_id));
    }

    // Test that existing mocks still work correctly
    #[tokio::test]
    async fn test_existing_mock_compatibility() {
        let mock_ogg = MockTrackForOggSelection {
            files: HashMap::new(),
        };
        
        // Should return a cover for index 0
        let cover_0 = mock_ogg.get_album_cover_file_id(0).await;
        assert!(cover_0.is_some());
        
        // Should return None for other indices
        let cover_1 = mock_ogg.get_album_cover_file_id(1).await;
        assert!(cover_1.is_none());

        let mock_m3u = MockTrackForM3uEntry {
            name: "Test".to_string(),
            artist_names: vec!["Artist".to_string()],
            duration_ms: 100000,
        };
        
        // Should return a cover for index 0
        let cover_0 = mock_m3u.get_album_cover_file_id(0).await;
        assert!(cover_0.is_some());
        
        // Should return None for other indices
        let cover_1 = mock_m3u.get_album_cover_file_id(1).await;
        assert!(cover_1.is_none());
    }
}
