use anyhow::Context;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Entry in an M3U playlist
#[derive(Debug, Clone)]
pub struct M3uEntry {
    /// Duration in seconds (-1 if unknown)
    pub duration: i32,
    /// Artist name
    pub artist: String,
    /// Track title
    pub title: String,
    /// Absolute path to the audio file
    pub file_path: PathBuf,
}

/// Calculate relative path from one file to another
fn calculate_relative_path(from: &Path, to: &Path) -> anyhow::Result<PathBuf> {
    // Get the parent directory of the 'from' file (where the M3U file will be)
    let from_dir = from
        .parent()
        .context("Failed to get parent directory of playlist file")?;

    // Calculate relative path from playlist directory to target file
    let relative =
        pathdiff::diff_paths(to, from_dir).context("Failed to calculate relative path")?;

    Ok(relative)
}

/// Write an M3U8 playlist file
pub fn write_m3u_playlist(
    playlist_path: &Path,
    entries: &[M3uEntry],
    spotify_url: Option<&str>,
) -> anyhow::Result<()> {
    let mut file = File::create(playlist_path).context("Failed to create M3U playlist file")?;

    // Write M3U8 header
    writeln!(file, "#EXTM3U")?;

    // Write Spotify URL as a comment if provided
    if let Some(url) = spotify_url {
        writeln!(file, "# Source: {}", url)?;
    }

    // Write each entry
    for entry in entries {
        // Calculate relative path from playlist to audio file
        let relative_path = calculate_relative_path(playlist_path, &entry.file_path)?;

        // Convert to forward slashes for M3U compatibility (works on all platforms)
        let path_str = relative_path.to_string_lossy().replace('\\', "/");

        // Write EXTINF line with duration and artist - title
        writeln!(
            file,
            "#EXTINF:{},{} - {}",
            entry.duration, entry.artist, entry.title
        )?;

        // Write file path
        writeln!(file, "{}", path_str)?;
    }

    Ok(())
}

/// Build an M3U entry from track metadata
pub async fn build_m3u_entry(metadata: &dyn crate::traits::TrackMetadataProvider, output_path: std::path::PathBuf) -> M3uEntry {
    let artist_names = metadata.artist_names().await;
    let artist = artist_names.first().cloned().unwrap_or_else(|| "Unknown Artist".to_string());

    M3uEntry {
        duration: (metadata.duration_ms().await / 1000) as i32,
        artist,
        title: metadata.name().await,
        file_path: output_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_calculate_relative_path_simple() {
        let from = PathBuf::from("/home/user/Music/Playlists/MyPlaylist.m3u");
        let to = PathBuf::from("/home/user/Music/Artist/Album/track.ogg");

        let relative = calculate_relative_path(&from, &to).unwrap();

        assert_eq!(relative, PathBuf::from("../Artist/Album/track.ogg"));
    }

    #[test]
    fn test_calculate_relative_path_nested() {
        let from = PathBuf::from("/home/user/Music/Playlists/Rock/MyPlaylist.m3u");
        let to = PathBuf::from("/home/user/Music/Artist/Album/track.ogg");

        let relative = calculate_relative_path(&from, &to).unwrap();

        assert_eq!(relative, PathBuf::from("../../Artist/Album/track.ogg"));
    }

    #[test]
    fn test_write_m3u_playlist() {
        let temp_dir = TempDir::new().unwrap();
        let playlist_path = temp_dir.path().join("test.m3u8");

        let entries = vec![
            M3uEntry {
                duration: 213,
                artist: "Artist One".to_string(),
                title: "Track One".to_string(),
                file_path: temp_dir.path().join("../Music/Artist/Album/track1.ogg"),
            },
            M3uEntry {
                duration: 180,
                artist: "Artist Two".to_string(),
                title: "Track Two".to_string(),
                file_path: temp_dir.path().join("../Music/Artist2/Album2/track2.ogg"),
            },
        ];

        write_m3u_playlist(&playlist_path, &entries, None).unwrap();

        // Read and verify content
        let content = fs::read_to_string(&playlist_path).unwrap();
        assert!(content.starts_with("#EXTM3U\n"));
        assert!(content.contains("#EXTINF:213,Artist One - Track One"));
        assert!(content.contains("#EXTINF:180,Artist Two - Track Two"));
        assert!(content.contains("track1.ogg"));
        assert!(content.contains("track2.ogg"));
    }

    #[test]
    fn test_m3u_paths_use_forward_slashes() {
        let temp_dir = TempDir::new().unwrap();
        let playlist_path = temp_dir.path().join("test.m3u8");

        let entries = vec![M3uEntry {
            duration: 100,
            artist: "Test".to_string(),
            title: "Test".to_string(),
            file_path: temp_dir.path().join("../Music/Artist/Album/track.ogg"),
        }];

        write_m3u_playlist(&playlist_path, &entries, None).unwrap();

        let content = fs::read_to_string(&playlist_path).unwrap();
        // Should use forward slashes even on Windows
        assert!(!content.contains("\\"));
    }
}
