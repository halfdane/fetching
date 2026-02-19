//! Input source handling and URI processing.
//!
//! Functions for reading URIs from files and parsing Spotify URIs
//! in various formats.

use anyhow::Context;

/// Read Spotify URIs from a file (one per line)
pub fn read_uris_from_file(path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let uris: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect();

    if uris.is_empty() {
        anyhow::bail!("No valid URIs found in file: {}", path.display());
    }

    Ok(uris)
}

/// Parse a Spotify URI from user input, handling various formats
pub fn parse_spotify_uri(uri_arg: &str) -> anyhow::Result<librespot_core::SpotifyUri> {
    use librespot_core::SpotifyUri;

    if uri_arg.starts_with("spotify:") {
        SpotifyUri::from_uri(uri_arg)
            .map_err(|_| anyhow::anyhow!("Invalid Spotify URI: {}", uri_arg))
    } else {
        // Assume it's a track ID if no prefix
        let uri_string = format!("spotify:track:{}", uri_arg);
        SpotifyUri::from_uri(&uri_string)
            .map_err(|_| anyhow::anyhow!("Invalid track ID: {}", uri_arg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_spotify_uri_with_track_prefix() {
        let uri = parse_spotify_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").unwrap();
        match uri {
            librespot_core::SpotifyUri::Track { .. } => {} // Success
            _ => panic!("Expected Track URI"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_with_album_prefix() {
        let uri = parse_spotify_uri("spotify:album:1A2GTWGtFfWp7KSQTwWOyo").unwrap();
        match uri {
            librespot_core::SpotifyUri::Album { .. } => {} // Success
            _ => panic!("Expected Album URI"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_with_playlist_prefix() {
        let uri = parse_spotify_uri("spotify:playlist:37i9dQZF1DX0XUsuxWHRQd").unwrap();
        match uri {
            librespot_core::SpotifyUri::Playlist { .. } => {} // Success
            _ => panic!("Expected Playlist URI"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_bare_id_assumes_track() {
        let uri = parse_spotify_uri("4uLU6hMCjMI75M1A2tKUQC").unwrap();
        match uri {
            librespot_core::SpotifyUri::Track { .. } => {} // Success
            _ => panic!("Expected Track URI for bare ID"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_invalid() {
        let result = parse_spotify_uri("invalid:stuff:here");
        assert!(result.is_err());
    }
}
