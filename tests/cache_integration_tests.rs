use spotify_player::{cache::*, traits::*};
use librespot_core::file_id::FileId;
use librespot_metadata::audio::AudioFileFormat;
use tokio::test;
use async_trait::async_trait;
use std::path::PathBuf;

// Integration tests for cache functionality using mocks
// These tests were moved from src/cache.rs to avoid crate boundary issues

#[derive(Debug)]
struct MockTrackForM3uEntry {
    pub name: String,
    pub artist_names: Vec<String>,
    pub duration_ms: u32,
}

#[async_trait]
impl TrackMetadataProvider for MockTrackForM3uEntry {
    async fn name(&self) -> String { self.name.clone() }
    async fn album_id(&self) -> String { "album".to_string() }
    async fn album_name(&self) -> String { "album".to_string() }
    async fn artist_names(&self) -> Vec<String> { self.artist_names.clone() }
    async fn duration_ms(&self) -> u32 { self.duration_ms }
    async fn date(&self) -> Option<String> { Some("2023".to_string()) }
    async fn track_number(&self) -> u32 { 1 }
    async fn get_file_id(&self, _format: &AudioFileFormat) -> Option<FileId> { None }
    
    async fn album_artist_names(&self) -> Vec<String> {
        vec!["Test Album Artist".to_string()]
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
        Some("Test Label".to_string())
    }
    
    async fn get_album_cover_file_id(&self, _index: usize) -> Option<FileId> {
        None
    }

    async fn alternative_uris(&self) -> Vec<String> {
        Vec::new() // No alternatives for this test mock
    }
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

