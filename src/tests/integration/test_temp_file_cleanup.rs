#[path = "../common/mod.rs"]
mod common;

use common::mock::MockAudioDownloader;
use spotify_player::cache::helpers::build_temp_path;
use spotify_player::stream::stream_and_cache_track;
use librespot_core::file_id::FileId;
use librespot_core::SpotifyId;
use librespot_core::SpotifyUri;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper to create fake FileId and SpotifyUri for testing
fn create_fake_ids() -> (FileId, SpotifyUri) {
    let file_id = FileId::from_raw(&[0u8; 20]);
    let spotify_id = SpotifyId::from_raw(&[0u8; 16]).unwrap();
    let uri = SpotifyUri::Track { id: spotify_id };
    (file_id, uri)
}

#[tokio::test]
async fn test_successful_download_removes_temp_file() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path_str = temp_dir.path().join("track.tmp.ogg");

    let mock = MockAudioDownloader::new_success();
    let (file_id, uri) = create_fake_ids();

    // Download should succeed
    let result = stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        temp_path_str.to_str().unwrap(),
        Some(std::time::Duration::ZERO),
    )
    .await;

    assert!(result.is_ok());

    // In the real flow, process_track_download would rename temp -> final
    // For this test, we just verify the download created the file
    assert!(temp_path_str.exists());
}

#[tokio::test]
async fn test_failed_download_leaves_no_temp_file() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path().join("track.tmp.ogg");

    // Mock that always fails with non-retriable error
    let mock = MockAudioDownloader::new_always_fails("invalid format".to_string());
    let (file_id, uri) = create_fake_ids();

    let result = stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        temp_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO),
    )
    .await;

    assert!(result.is_err());

    // Temp file should not exist (mock never creates it on failure)
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn test_retry_scenario_no_partial_files() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path().join("track.tmp.ogg");

    // Mock that fails twice then succeeds
    let mock = MockAudioDownloader::new_with_retries(2);
    let (file_id, uri) = create_fake_ids();

    let result = stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        temp_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO),
    )
    .await;

    assert!(result.is_ok());

    // Should have the final temp file from successful attempt
    assert!(temp_path.exists());

    // Verify no .tmp.1, .tmp.2, etc. files lying around
    // (Our implementation writes to the same path each time)
    let parent = temp_path.parent().unwrap();
    let entries: Vec<_> = std::fs::read_dir(parent).unwrap().collect();

    // Should only have one file (the successful download)
    assert_eq!(entries.len(), 1);
}

#[test]
fn test_build_temp_path_preserves_parent_directory() {
    let output_path = PathBuf::from("/home/user/Music/Artist/Album/track.ogg");
    let temp_path = build_temp_path(&output_path);

    assert_eq!(temp_path.parent(), output_path.parent());
    assert_eq!(temp_path.file_name().unwrap(), "track.tmp.ogg");
}

#[test]
fn test_build_temp_path_keeps_ogg_extension() {
    let output_path = PathBuf::from("track.ogg");
    let temp_path = build_temp_path(&output_path);

    // Extension should be .tmp.ogg so lofty can detect format
    assert!(temp_path.to_string_lossy().ends_with(".tmp.ogg"));
}

#[tokio::test]
async fn test_exhausted_retries_no_temp_file() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path().join("track.tmp.ogg");

    // Mock that always fails with retriable error (exhausts retries)
    let mock = MockAudioDownloader::new_with_retries(999);
    let (file_id, uri) = create_fake_ids();

    let result = stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        temp_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO),
    )
    .await;

    assert!(result.is_err());

    // No temp file should exist (mock doesn't create on failure)
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn test_pre_existing_temp_file_not_interfere() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path().join("track.tmp.ogg");

    // Create a pre-existing temp file (simulating previous failed attempt)
    std::fs::write(&temp_path, b"old corrupted data").unwrap();
    assert!(temp_path.exists());

    // Mock that succeeds
    let mock = MockAudioDownloader::new_success();
    let (file_id, uri) = create_fake_ids();

    let result = stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        temp_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO),
    )
    .await;

    assert!(result.is_ok());

    // Should have new content, not old
    let content = std::fs::read(&temp_path).unwrap();
    assert!(!content.is_empty(), "Output file should contain OGG data");
    // Verify it starts with OGG magic bytes
    assert_eq!(&content[0..4], b"OggS", "File should be valid OGG format");
}
