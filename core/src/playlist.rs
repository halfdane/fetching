//! M3U8 playlist writing and composite cover art generation.
//!
//! # Cover art layouts
//!
//! | # covers | Layout |
//! |----------|--------|
//! | 1        | Single image scaled to 600×600 |
//! | 2        | Left/right halves (300×600 each) |
//! | 3        | Left half (cover 1, 300×600) + stacked right (covers 2–3, 300×300 each) |
//! | 4        | 2×2 grid (300×300 each) |
//! | 5        | 2×2 grid of covers 2–5 + cover 1 centred overlay with 10 px white border |

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use tracing::warn;

use crate::container::TrackCollection;

// ---------------------------------------------------------------------------
// Track entry for M3U8
// ---------------------------------------------------------------------------

/// A successfully-downloaded track to include in a playlist.
pub struct TrackEntry {
    /// Absolute path to the downloaded audio file.
    pub final_path: PathBuf,
    /// Track title (written into `#EXTINF`).
    pub title: String,
    /// Duration in milliseconds (written as seconds in `#EXTINF`).
    pub duration_ms: i32,
}

// ---------------------------------------------------------------------------
// M3U8 writing
// ---------------------------------------------------------------------------

/// Write an [Extended M3U](https://en.wikipedia.org/wiki/M3U#Extended_M3U)
/// playlist at `m3u8_path`.
///
/// The file begins with a collection-level header block:
/// ```text
/// #EXTM3U
/// #PLAYLIST:<title>
/// #EXTART:<primary artist>          (omitted when artist list is empty)
/// # Spotify-URI: spotify:album:…
/// # UPC: 00602…                     (omitted when absent)
/// # Date: 2024-03-15                (omitted when absent)
/// ```
///
/// All track paths are written relative to the playlist's parent directory so
/// the playlist remains valid after the root is renamed or moved.
pub fn write_m3u8(
    m3u8_path: &Path,
    collection: &TrackCollection,
    tracks: &[TrackEntry],
) -> anyhow::Result<()> {
    let base_dir = m3u8_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("m3u8 path has no parent directory"))?;
    std::fs::create_dir_all(base_dir)?;

    let file = std::fs::File::create(m3u8_path)?;
    let mut out = std::io::BufWriter::new(file);

    // ── Collection header ────────────────────────────────────────────────────
    writeln!(out, "#EXTM3U")?;
    writeln!(out, "#PLAYLIST:{}", collection.title)?;

    if let Some(artist) = collection.artists.first() {
        writeln!(out, "#EXTART:{}", artist)?;
    }

    writeln!(out, "# Spotify-URI: {}", collection.uri_str)?;

    if let Some(upc) = &collection.upc {
        writeln!(out, "# UPC: {}", upc)?;
    }

    if let Some(date) = &collection.date {
        writeln!(out, "# Date: {}", date)?;
    }

    writeln!(out)?; // blank line between header and track list

    // ── Track entries ────────────────────────────────────────────────────────
    for entry in tracks {
        let rel = relative_path(base_dir, &entry.final_path);
        let duration_secs = entry.duration_ms / 1000;
        writeln!(out, "#EXTINF:{},{}", duration_secs, entry.title)?;
        writeln!(out, "{}", rel.display())?;
    }

    Ok(())
}

/// Compute the relative path from `from_dir` to `to_file`.
///
/// Both paths should be absolute (or share the same working-directory base).
pub fn relative_path(from_dir: &Path, to_file: &Path) -> PathBuf {
    let base: Vec<_> = from_dir.components().collect();
    let target: Vec<_> = to_file.components().collect();

    let common = base
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();
    for _ in &base[common..] {
        result.push("..");
    }
    for c in &target[common..] {
        result.push(c.as_os_str());
    }
    result
}

// ---------------------------------------------------------------------------
// Composite cover art
// ---------------------------------------------------------------------------

const TILE: u32 = 300;
const CANVAS: u32 = 600;

/// Build a square JPEG composite from 1–5 cover images.
///
/// See the module-level doc-table for the exact layouts.  Images that cannot
/// be decoded are silently skipped; an error is returned only if *all* images
/// fail to load.
pub fn composite_cover(cover_images: &[Vec<u8>]) -> anyhow::Result<Vec<u8>> {
    if cover_images.is_empty() {
        anyhow::bail!("composite_cover: no cover images provided");
    }

    let mut tiles: Vec<DynamicImage> = Vec::new();
    for (i, bytes) in cover_images.iter().enumerate() {
        match image::load_from_memory(bytes) {
            Ok(img) => tiles.push(img),
            Err(e) => warn!("Failed to decode cover image {}: {}", i, e),
        }
    }

    if tiles.is_empty() {
        anyhow::bail!("composite_cover: failed to decode any cover images");
    }

    let canvas: RgbImage = match tiles.len() {
        1 => tiles[0]
            .resize_exact(CANVAS, CANVAS, image::imageops::FilterType::Lanczos3)
            .into_rgb8(),

        2 => layout_two(&tiles),
        3 => layout_three(&tiles),
        4 => layout_grid_2x2(&tiles[..4]),

        _ => {
            // 5+ covers: 2×2 grid of covers[1..5] + cover[0] overlay
            let grid_tiles = &tiles[1..5.min(tiles.len())];
            // Pad with repeats if we somehow got fewer than 4 non-zero tiles
            let mut four: Vec<DynamicImage> = grid_tiles.to_vec();
            while four.len() < 4 {
                four.push(tiles[0].clone());
            }
            let mut canvas = layout_grid_2x2(&four);
            overlay_centered_with_border(&mut canvas, &tiles[0]);
            canvas
        }
    };

    encode_jpeg(canvas)
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

fn layout_two(tiles: &[DynamicImage]) -> RgbImage {
    let mut canvas = RgbImage::new(CANVAS, CANVAS);
    for (i, tile) in tiles.iter().enumerate().take(2) {
        // Each cover occupies a TILE-wide vertical strip at full height
        let resized = tile
            .resize_exact(TILE, CANVAS, image::imageops::FilterType::Lanczos3)
            .to_rgb8();
        image::imageops::replace(&mut canvas, &resized, (i as u64 * TILE as u64) as i64, 0);
    }
    canvas
}

fn layout_three(tiles: &[DynamicImage]) -> RgbImage {
    let mut canvas = RgbImage::new(CANVAS, CANVAS);

    // Cover 0: full-height left half
    let left = tiles[0]
        .resize_exact(TILE, CANVAS, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    image::imageops::replace(&mut canvas, &left, 0, 0);

    // Covers 1–2: stacked in the right half
    for (i, tile) in tiles[1..3.min(tiles.len())].iter().enumerate() {
        let resized = tile
            .resize_exact(TILE, TILE, image::imageops::FilterType::Lanczos3)
            .to_rgb8();
        let y = (i as u32 * TILE) as i64;
        image::imageops::replace(&mut canvas, &resized, TILE as i64, y);
    }
    canvas
}

fn layout_grid_2x2(tiles: &[DynamicImage]) -> RgbImage {
    let mut canvas = RgbImage::new(CANVAS, CANVAS);
    for (idx, tile) in tiles.iter().enumerate().take(4) {
        let resized = tile
            .resize_exact(TILE, TILE, image::imageops::FilterType::Lanczos3)
            .to_rgb8();
        let row = (idx / 2) as i64;
        let col = (idx % 2) as i64;
        image::imageops::replace(
            &mut canvas,
            &resized,
            col * TILE as i64,
            row * TILE as i64,
        );
    }
    canvas
}

/// Paint a white-bordered square of `overlay_img` centred on `canvas`.
///
/// The border is 10 px on each side; the image itself is scaled to 220×220,
/// giving a total overlay footprint of 240×240 at (180, 180).
fn overlay_centered_with_border(canvas: &mut RgbImage, overlay_img: &DynamicImage) {
    const BORDER: u32 = 10;
    const IMG_SIZE: u32 = 220;
    const TOTAL: u32 = IMG_SIZE + 2 * BORDER; // 240
    const OFFSET: u32 = (CANVAS - TOTAL) / 2;  // 180

    // Fill white rectangle
    let white = Rgb([255u8, 255u8, 255u8]);
    for y in OFFSET..(OFFSET + TOTAL) {
        for x in OFFSET..(OFFSET + TOTAL) {
            canvas.put_pixel(x, y, white);
        }
    }

    // Draw the image inside the border
    let resized = overlay_img
        .resize_exact(IMG_SIZE, IMG_SIZE, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    image::imageops::replace(
        canvas,
        &resized,
        (OFFSET + BORDER) as i64,
        (OFFSET + BORDER) as i64,
    );
}

fn encode_jpeg(canvas: RgbImage) -> anyhow::Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(canvas).write_to(&mut buf, ImageFormat::Jpeg)?;
    Ok(buf.into_inner())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn relative_path_same_dir() {
        let base = PathBuf::from("/a/b/c");
        let file = PathBuf::from("/a/b/c/track.ogg");
        assert_eq!(relative_path(&base, &file), PathBuf::from("track.ogg"));
    }

    #[test]
    fn relative_path_sibling_dir() {
        let base = PathBuf::from("/music/Playlists/My List");
        let file = PathBuf::from("/music/Artist/Album/01 - Song.ogg");
        assert_eq!(
            relative_path(&base, &file),
            PathBuf::from("../../Artist/Album/01 - Song.ogg"),
        );
    }

    #[test]
    fn relative_path_child_dir() {
        let base = PathBuf::from("/music");
        let file = PathBuf::from("/music/Artist/Album/01 - Song.ogg");
        assert_eq!(
            relative_path(&base, &file),
            PathBuf::from("Artist/Album/01 - Song.ogg"),
        );
    }

    #[test]
    fn composite_cover_rejects_empty() {
        assert!(composite_cover(&[]).is_err());
    }
}
