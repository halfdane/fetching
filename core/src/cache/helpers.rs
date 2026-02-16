//! Utility functions for caching operations.
//!
//! Pure functions that don't depend on external services or async operations.

use std::path::{Path, PathBuf};

/// Extract artist name from a list of artist names, returning "Unknown Artist" if empty
pub fn get_artist_name_from_vec(artists: &[String]) -> String {
    if !artists.is_empty() {
        artists[0].clone()
    } else {
        "Unknown Artist".to_string()
    }
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