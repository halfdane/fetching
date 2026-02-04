use spotify_player::collection::build_m3u_entry;
use spotify_player::m3u::write_m3u_playlist;
use spotify_player::traits::TrackMetadataProvider;
use std::path::PathBuf;
use tempfile::TempDir;

#[derive(Debug)]
struct TestTrackProvider {
    pub name: String,
    pub artist_names: Vec<String>,
    pub duration_ms: u32,
}

#[async_trait::async_trait]
impl TrackMetadataProvider for TestTrackProvider {
    async fn id(&self) -> String { "test".to_string() }
    async fn name(&self) -> String { self.name.clone() }
    async fn album_id(&self) -> String { "album".to_string() }
    async fn album_name(&self) -> String { "album".to_string() }
    async fn artist_names(&self) -> Vec<String> { self.artist_names.clone() }
    async fn duration_ms(&self) -> u32 { self.duration_ms }
    async fn year(&self) -> i32 { 2023 }
    async fn track_number(&self) -> u32 { 1 }
    async fn get_file_id(&self, _format: &librespot_metadata::audio::AudioFileFormat) -> Option<librespot_core::file_id::FileId> { None }
    
    async fn get_album_cover_file_id(&self, index: usize) -> Option<librespot_core::file_id::FileId> {
        if index == 0 {
            Some(librespot_core::file_id::FileId::from_raw(&[1u8; 16]))
        } else {
            None
        }
    }
}

#[tokio::test]
async fn test_build_m3u_entry_integration() {
    let provider = TestTrackProvider {
        name: "Integration Test Song".to_string(),
        artist_names: vec!["Integration Artist".to_string()],
        duration_ms: 300000, // 5 minutes
    };

    let output_path = PathBuf::from("/test/path/song.ogg");
    let entry = build_m3u_entry(&provider, output_path).await;

    assert_eq!(entry.title, "Integration Test Song");
    assert_eq!(entry.artist, "Integration Artist");
    assert_eq!(entry.duration, 300);
}

#[tokio::test]
async fn test_m3u_playlist_with_mock_provider() {
    let temp_dir = TempDir::new().unwrap();
    let playlist_path = temp_dir.path().join("test_playlist.m3u8");

    let entries = vec![
        build_m3u_entry(
            &TestTrackProvider {
                name: "Song One".to_string(),
                artist_names: vec!["Artist A".to_string()],
                duration_ms: 200000,
            },
            temp_dir.path().join("song1.ogg")
        ).await,
        build_m3u_entry(
            &TestTrackProvider {
                name: "Song Two".to_string(),
                artist_names: vec!["Artist B".to_string()],
                duration_ms: 250000,
            },
            temp_dir.path().join("song2.ogg")
        ).await,
    ];

    write_m3u_playlist(&playlist_path, &entries, Some("spotify:playlist:test")).unwrap();

    let content = std::fs::read_to_string(&playlist_path).unwrap();
    assert!(content.contains("#EXTINF:200,Artist A - Song One"));
    assert!(content.contains("#EXTINF:250,Artist B - Song Two"));
    assert!(content.contains("song1.ogg"));
    assert!(content.contains("song2.ogg"));
    assert!(content.contains("# Source: spotify:playlist:test"));
}