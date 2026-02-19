//! Path and filename construction utilities.
//!
//! Functions for building file paths and directory structures
//! for organizing downloaded tracks.

use std::path::PathBuf;

use crate::metadata::validation::sanitize;
use crate::traits::TrackMetadataProvider;

/// Build the output path for a track (directory structure + filename)
///
/// # Errors
///
/// Returns error if directory creation fails or path cannot be constructed.
pub async fn build_track_path<T: TrackMetadataProvider + ?Sized>(
    track: &T,
    base_music_dir: &str,
) -> anyhow::Result<PathBuf> {
    let date = track.date().await;
    let year = date
        .as_ref()
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse::<i32>().ok())
        .unwrap_or(0);
    let artist_names = track.artist_names().await;
    let artist_name = sanitize(&artist_names.join(" & "));
    let album_name = sanitize(&track.album_name().await);
    let track_title = sanitize(&track.name().await);
    let track_num = track.track_number().await;

    let mut dir_path = PathBuf::from(base_music_dir);
    dir_path.push(&artist_name);
    dir_path.push(format!("{} - {}", year, album_name));
    std::fs::create_dir_all(&dir_path)?;

    let filename = format!("{:02} - {}.ogg", track_num, track_title);

    dir_path.push(filename);
    Ok(dir_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TrackMetadataProvider;
    use async_trait::async_trait;

    // Mock implementation for testing build_track_path
    #[derive(Debug)]
    struct MockTrack {
        pub name: String,
        pub artist_names: Vec<String>,
        pub album_name: String,
        pub year: i32,
        pub track_number: u32,
    }

    #[async_trait]
    impl TrackMetadataProvider for MockTrack {
        async fn name(&self) -> String {
            self.name.clone()
        }

        async fn artist_names(&self) -> Vec<String> {
            self.artist_names.clone()
        }

        async fn album_name(&self) -> String {
            self.album_name.clone()
        }

        async fn album_id(&self) -> String {
            "mock_album_id".to_string()
        }

        async fn date(&self) -> Option<String> {
            if self.year > 0 {
                Some(self.year.to_string())
            } else {
                None
            }
        }

        async fn track_number(&self) -> u32 {
            self.track_number
        }

        async fn duration_ms(&self) -> u32 {
            180000 // 3 minutes
        }

        async fn album_artist_names(&self) -> Vec<String> {
            vec!["Mock Album Artist".to_string()]
        }
        async fn disc_number(&self) -> u32 {
            1
        }
        async fn genres(&self) -> Vec<String> {
            vec!["Rock".to_string()]
        }
        async fn isrc(&self) -> Option<String> {
            Some("US1234567890".to_string())
        }
        async fn label(&self) -> Option<String> {
            Some("Mock Label".to_string())
        }

        async fn get_file_id(
            &self,
            _format: &librespot_metadata::audio::AudioFileFormat,
        ) -> Option<librespot_core::file_id::FileId> {
            None
        }

        async fn get_album_cover_file_id(
            &self,
            index: usize,
        ) -> Option<librespot_core::file_id::FileId> {
            if index == 0 {
                Some(librespot_core::file_id::FileId::from_raw(&[1u8; 16]))
            } else {
                None
            }
        }

        async fn alternative_uris(&self) -> Vec<String> {
            Vec::new() // No alternatives for this test mock
        }
    }

    #[tokio::test]
    async fn test_build_track_path_without_prefix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_music_dir = temp_dir.path().to_str().unwrap();

        let mock_track = MockTrack {
            name: "Test Track".to_string(),
            artist_names: vec!["Test Artist".to_string()],
            album_name: "Test Album".to_string(),
            year: 2023,
            track_number: 5,
        };

        let result = build_track_path(&mock_track, base_music_dir).await.unwrap();

        let expected_filename = "2023_Test_Artist_Test_Album_005_Test_Track.ogg";
        assert_eq!(result.file_name().unwrap(), expected_filename);

        // Check that the directory structure was created
        let expected_dir = temp_dir.path().join("Test_Artist").join("Test_Album");
        assert!(expected_dir.exists());
        assert!(result.parent().unwrap() == expected_dir);
    }

    #[tokio::test]
    async fn test_build_track_path_with_prefix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_music_dir = temp_dir.path().to_str().unwrap();

        let mock_track = MockTrack {
            name: "Another Track".to_string(),
            artist_names: vec!["Another Artist".to_string()],
            album_name: "Another Album".to_string(),
            year: 2024,
            track_number: 1,
        };

        // Prefix is no longer supported, so this test is obsolete.
        let result = build_track_path(&mock_track, base_music_dir).await.unwrap();

        let expected_filename = "PREFIX_2024_Another_Artist_Another_Album_001_Another_Track.ogg";
        assert_eq!(result.file_name().unwrap(), expected_filename);
    }

    #[tokio::test]
    async fn test_build_track_path_multiple_artists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_music_dir = temp_dir.path().to_str().unwrap();

        let mock_track = MockTrack {
            name: "Collaboration Track".to_string(),
            artist_names: vec!["Artist One".to_string(), "Artist Two".to_string()],
            album_name: "Collaboration Album".to_string(),
            year: 2022,
            track_number: 10,
        };

        let result = build_track_path(&mock_track, base_music_dir).await.unwrap();

        let expected_filename =
            "2022_Artist_One_Artist_Two_Collaboration_Album_010_Collaboration_Track.ogg";
        assert_eq!(result.file_name().unwrap(), expected_filename);
    }

    #[tokio::test]
    async fn test_build_track_path_special_characters() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_music_dir = temp_dir.path().to_str().unwrap();

        let mock_track = MockTrack {
            name: "Track: With? Special*Chars!".to_string(),
            artist_names: vec!["Artist/With\\Bad<Chars>".to_string()],
            album_name: "Album: Deluxe|Edition?".to_string(),
            year: 2021,
            track_number: 2,
        };

        let result = build_track_path(&mock_track, base_music_dir).await.unwrap();

        let expected_filename =
            "2021_Artist_With_Bad_Chars_Album_Deluxe_Edition_002_Track_With_Special_Chars.ogg";
        assert_eq!(result.file_name().unwrap(), expected_filename);
    }
}
