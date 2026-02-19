//! Core track processing and caching logic.
//!
//! Functions for downloading tracks, adding metadata, and managing the
//! caching workflow with error handling and cleanup.

use librespot_core::SpotifyUri;
use librespot_metadata::audio::AudioFileFormat;
use std::path::Path;
use tracing::{debug, error, info, warn};

use crate::cache::helpers::build_temp_path;
use crate::error::DownloadError;
use crate::metadata::{TrackMetadata, write_ogg_tags};
use crate::stream::stream_and_cache_track;
use crate::traits::TrackMetadataProvider;

/// Get the best quality OGG Vorbis file ID and format for a track
/// Returns (FileId, AudioFileFormat) if found
async fn select_best_ogg_file<T: crate::traits::TrackMetadataProvider>(
    track: &T,
) -> Option<(librespot_core::file_id::FileId, AudioFileFormat)> {
    // Try formats in order of quality (highest to lowest)
    let formats = [
        AudioFileFormat::OGG_VORBIS_320,
        AudioFileFormat::OGG_VORBIS_160,
        AudioFileFormat::OGG_VORBIS_96,
    ];

    for format in &formats {
        if let Some(file_id) = track.get_file_id(format).await {
            return Some((file_id, *format));
        }
    }

    None
}

/// Get a track with OGG Vorbis format, selecting the highest quality from original and alternatives
/// Returns (Track, FileId) where the track has the best available OGG format
pub async fn get_track_with_ogg_format(
    track_fetcher: &dyn crate::traits::TrackFetcher,
    uri: &SpotifyUri,
) -> anyhow::Result<(
    Box<dyn crate::traits::TrackMetadataProvider>,
    librespot_core::file_id::FileId,
)> {
    let track = track_fetcher.fetch_track(uri).await?;
    let provider = crate::implementations::OwnedLibrespotTrackProvider {
        track: track.clone(),
    };

    // Collect all candidates: original track + all alternatives with their OGG format
    let mut candidates: Vec<(
        Box<dyn crate::traits::TrackMetadataProvider>,
        librespot_core::file_id::FileId,
        AudioFileFormat,
        String,
    )> = Vec::new();

    // Check original track
    if let Some((file_id, format)) = select_best_ogg_file(&provider).await {
        // Early termination: if original has highest quality (320), no need to check alternatives
        if format == AudioFileFormat::OGG_VORBIS_320 {
            debug!(
                "Track '{}' has OGG_VORBIS_320 in original, skipping alternatives",
                provider.track.name
            );
            return Ok((
                Box::new(crate::implementations::OwnedLibrespotTrackProvider {
                    track: track.clone(),
                }),
                file_id,
            ));
        }
        candidates.push((
            Box::new(crate::implementations::OwnedLibrespotTrackProvider {
                track: track.clone(),
            }),
            file_id,
            format,
            "original".to_string(),
        ));
    }

    // Check all alternatives if original doesn't exist or doesn't have best quality
    let alternative_uris = provider.alternative_uris().await;
    if candidates.is_empty() || !alternative_uris.is_empty() {
        debug!(
            "Track '{}' checking {} alternatives for better quality",
            provider.track.name,
            alternative_uris.len()
        );

        for (i, alt_uri_str) in alternative_uris.iter().enumerate() {
            let alt_uri = SpotifyUri::from_uri(alt_uri_str)?;
            match track_fetcher.fetch_track(&alt_uri).await {
                Ok(alt_track) => {
                    let alt_provider = crate::implementations::OwnedLibrespotTrackProvider {
                        track: alt_track.clone(),
                    };
                    if let Some((file_id, format)) = select_best_ogg_file(&alt_provider).await {
                        candidates.push((
                            Box::new(crate::implementations::OwnedLibrespotTrackProvider {
                                track: alt_track.clone(),
                            }),
                            file_id,
                            format,
                            format!("alternative {}", i + 1),
                        ));
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
        anyhow::bail!(
            "Track '{}' not available in OGG Vorbis format (tried {} alternatives)",
            track.name,
            alternative_uris.len()
        )
    }

    // Sort by format quality (320 > 160 > 96)
    candidates.sort_by_key(|(_, _, format, _)| match format {
        AudioFileFormat::OGG_VORBIS_320 => 0,
        AudioFileFormat::OGG_VORBIS_160 => 1,
        AudioFileFormat::OGG_VORBIS_96 => 2,
        _ => 3,
    });

    let (best_track, file_id, format, source) = candidates.into_iter().next().unwrap();
    info!(
        "Selected {:?} from {} for track '{}'",
        format,
        source,
        best_track.name().await
    );

    Ok((best_track, file_id))
}

async fn cache_track_cover_art(
    image_downloader: &dyn crate::traits::ImageDownloader,
    metadata: &dyn crate::traits::TrackMetadataProvider,
) -> Option<Vec<u8>> {
    if let Some(file_id) = metadata.get_album_cover_file_id(0).await {
        tracing::info!("Fetching cover art for track '{}'", metadata.name().await);
        let _ = std::io::Write::flush(&mut std::io::stdout());
        match image_downloader.download_cover(&file_id).await {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                // Print error inline without newline to keep on same line as track
                println!(" ❌[cover]");
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
        println!(" ❌");
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
        println!(" ❌");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        error!("Failed to finalize file: {}", e);
        return Err(e.into());
    }
    Ok(())
}

/// Process a single track: check existence, cache, add metadata
pub async fn process_track_cache(
    _track_fetcher: &dyn crate::traits::TrackFetcher,
    audio_downloader: &dyn crate::traits::AudioDownloader,
    image_downloader: &dyn crate::traits::ImageDownloader,
    track_provider: &dyn crate::traits::TrackMetadataProvider,
    track_uri: &SpotifyUri,
    output_path: &Path,
    file_id: &librespot_core::file_id::FileId,
) -> anyhow::Result<()> {
    // Check if file already exists
    if output_path.exists() {
        tracing::info!(
            "Track '{}' already cached at {}, skipping download",
            track_provider.name().await,
            output_path.display()
        );
        return Ok(());
    }

    let temp_path = build_temp_path(output_path);

    tracing::info!(
        "Starting to fetch track '{}' to {}",
        track_provider.name().await,
        temp_path.display()
    );

    // Clean up any existing temp file
    if temp_path.exists() {
        if let Err(e) = std::fs::remove_file(&temp_path) {
            warn!("Failed to remove existing temp file: {}", e);
        }
    }

    let temp_path_str = match temp_path.to_str() {
        Some(s) => s,
        None => {
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
        println!(" ❌");
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

    tracing::info!(
        "Track '{}' fetched, tagged and stored successfully at {}",
        track_provider.name().await,
        output_path.display()
    );
    Ok(())
}
