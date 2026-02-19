//! Image and cover art handling for caching operations.
//!
//! Functions for downloading, saving, and processing album cover images,
//! including collage creation for playlists.

use std::collections::HashSet;
use std::path::Path;
use tracing::warn;

/// Collect unique album covers for collage creation (up to 4 unique albums)
pub async fn collect_album_cover(
    image_downloader: &dyn crate::traits::ImageDownloader,
    metadata: &dyn crate::traits::TrackMetadataProvider,
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
        tracing::info!("Saving cover art to: {}", cover_path.display());
        std::fs::write(&cover_path, bytes)?;
        tracing::info!("Cover art saved successfully");
    } else if !unique_album_covers.is_empty() {
        tracing::info!(
            "Creating cover collage from {} album covers",
            unique_album_covers.len()
        );
        let collage_bytes = create_cover_collage(unique_album_covers)?;
        std::fs::write(&cover_path, collage_bytes)?;
        tracing::info!("Cover collage saved successfully");
    } else {
        tracing::info!("No cover art bytes to save");
    }

    Ok(())
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
