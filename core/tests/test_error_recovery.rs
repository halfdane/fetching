
mod mock;
use mock::MockAudioDownloader;
use fetching_core::stream::is_retriable_error;
use fetching_core::traits::AudioDownloader;
use librespot_core::file_id::FileId;
use librespot_core::SpotifyId;
use librespot_core::SpotifyUri;
use tempfile::TempDir;

// Helper to create fake FileId and SpotifyUri for testing
fn create_fake_ids() -> (FileId, SpotifyUri) {
    let file_id = FileId::from_raw(&[0u8; 20]);
    let spotify_id = SpotifyId::from_raw(&[0u8; 16]).unwrap();
    let uri = SpotifyUri::Track { id: spotify_id };
    (file_id, uri)
}

#[test]
fn test_is_retriable_error() {
    assert!(is_retriable_error("audio key error occurred"));
    assert!(is_retriable_error("Service unavailable - try again"));
    assert!(is_retriable_error("timeout exceeded"));
    assert!(is_retriable_error("Deadline expired"));

    assert!(!is_retriable_error("invalid track format"));
    assert!(!is_retriable_error("file not found"));
    assert!(!is_retriable_error("permission denied"));
}

#[tokio::test]
async fn test_mock_downloader_success() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test.ogg");

    let mock = MockAudioDownloader::new_success();
    let (file_id, uri) = create_fake_ids();

    let result = mock
        .stream_track(&file_id, &uri, output_path.to_str().unwrap())
        .await;

    assert!(result.is_ok());
    assert!(output_path.exists());

    let attempts = mock.download_attempts.lock().unwrap();
    assert_eq!(attempts.len(), 1);

    let successful = mock.successful_downloads.lock().unwrap();
    assert_eq!(successful.len(), 1);
}

#[tokio::test]
async fn test_mock_downloader_retry_then_success() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test.ogg");

    let mock = MockAudioDownloader::new_with_retries(2);
    let (file_id, uri) = create_fake_ids();

    // First attempt - should fail
    let result1 = mock
        .stream_track(&file_id, &uri, output_path.to_str().unwrap())
        .await;
    assert!(result1.is_err());
    assert_eq!(result1.unwrap_err().to_string(), "Service unavailable");

    // Second attempt - should fail
    let result2 = mock
        .stream_track(&file_id, &uri, output_path.to_str().unwrap())
        .await;
    assert!(result2.is_err());

    // Third attempt - should succeed
    let result3 = mock
        .stream_track(&file_id, &uri, output_path.to_str().unwrap())
        .await;
    assert!(result3.is_ok());
    assert!(output_path.exists());

    let attempts = mock.download_attempts.lock().unwrap();
    assert_eq!(attempts.len(), 3);

    let successful = mock.successful_downloads.lock().unwrap();
    assert_eq!(successful.len(), 1);
}

#[tokio::test]
async fn test_mock_downloader_always_fails() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test.ogg");

    let mock = MockAudioDownloader::new_always_fails("Network error".to_string());
    let (file_id, uri) = create_fake_ids();

    let result = mock
        .stream_track(&file_id, &uri, output_path.to_str().unwrap())
        .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "Network error");
    assert!(!output_path.exists());

    let attempts = mock.download_attempts.lock().unwrap();
    assert_eq!(attempts.len(), 1);

    let successful = mock.successful_downloads.lock().unwrap();
    assert_eq!(successful.len(), 0);
}

#[tokio::test]
async fn test_download_with_retry_succeeds_after_failures() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test_track.ogg");

    // Mock that fails twice, then succeeds
    let mock = MockAudioDownloader::new_with_retries(2);
    let (file_id, uri) = create_fake_ids();

    let result = fetching_core::stream::stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        output_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO), // No delay in tests
    )
    .await;

    // Should succeed after retries
    assert!(result.is_ok());
    assert!(output_path.exists());

    // Should have attempted 3 times (2 failures + 1 success)
    let attempts = mock.download_attempts.lock().unwrap();
    assert_eq!(attempts.len(), 3);

    let successful = mock.successful_downloads.lock().unwrap();
    assert_eq!(successful.len(), 1);
}

#[tokio::test]
async fn test_download_fails_immediately_on_non_retriable_error() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test_track.ogg");

    // Mock that always fails with non-retriable error
    let mock = MockAudioDownloader::new_always_fails("invalid format".to_string());
    let (file_id, uri) = create_fake_ids();

    let result = fetching_core::stream::stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        output_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO), // No delay in tests
    )
    .await;

    // Should fail immediately without retries
    assert!(result.is_err());
    assert!(!output_path.exists());

    // Should have only attempted once (non-retriable error)
    let attempts = mock.download_attempts.lock().unwrap();
    assert_eq!(attempts.len(), 1);
}

#[tokio::test]
async fn test_download_exhausts_all_retries() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test_track.ogg");

    // Mock that always fails with retriable error
    let mock = MockAudioDownloader::new_with_retries(999);
    let (file_id, uri) = create_fake_ids();

    let result = fetching_core::stream::stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        output_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO), // No delay in tests
    )
    .await;

    // Should fail after exhausting retries
    assert!(result.is_err());
    assert!(!output_path.exists());

    // Should have attempted MAX_RETRIES times (3)
    let attempts = mock.download_attempts.lock().unwrap();
    assert_eq!(attempts.len(), 5);

    let successful = mock.successful_downloads.lock().unwrap();
    assert_eq!(successful.len(), 0);
}

#[tokio::test]
async fn test_download_succeeds_on_first_try() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test_track.ogg");

    // Mock that succeeds immediately
    let mock = MockAudioDownloader::new_success();
    let (file_id, uri) = create_fake_ids();

    let result = fetching_core::stream::stream_and_cache_track(
        &mock,
        &file_id,
        &uri,
        output_path.to_str().unwrap(),
        Some(std::time::Duration::ZERO), // No delay in tests
    )
    .await;

    // Should succeed
    assert!(result.is_ok());
    assert!(output_path.exists());

    // Should have only attempted once
    let attempts = mock.download_attempts.lock().unwrap();
    assert_eq!(attempts.len(), 1);

    let successful = mock.successful_downloads.lock().unwrap();
    assert_eq!(successful.len(), 1);
}
