/// Mock implementations for testing
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use spotify_player::traits::AudioDownloader;
use librespot_core::file_id::FileId;
use std::collections::HashMap;
use librespot_metadata::audio::AudioFileFormat;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use librespot_core::SpotifyUri;
use spotify_player::traits::TrackMetadataProvider;

/// Mock audio downloader for testing retry logic and error scenarios
pub struct MockAudioDownloader {
    /// Number of times to fail before succeeding (decrements on each call)
    pub failures_before_success: AtomicU32,
    /// Optional custom error message to return
    pub error_to_return: Option<String>,
    /// Records all attempted downloads
    pub download_attempts: Mutex<Vec<String>>,
    /// Records successfully downloaded files
    pub successful_downloads: Mutex<Vec<String>>,
}

impl MockAudioDownloader {
    /// Create a mock that always succeeds
    pub fn new_success() -> Self {
        Self {
            failures_before_success: AtomicU32::new(0),
            error_to_return: None,
            download_attempts: Mutex::new(Vec::new()),
            successful_downloads: Mutex::new(Vec::new()),
        }
    }

    /// Create a mock that fails N times then succeeds
    pub fn new_with_retries(failures: u32) -> Self {
        Self {
            failures_before_success: AtomicU32::new(failures),
            error_to_return: Some("Service unavailable".to_string()), // Retriable
            download_attempts: Mutex::new(Vec::new()),
            successful_downloads: Mutex::new(Vec::new()),
        }
    }

    /// Create a mock that always fails with the given error
    pub fn new_always_fails(error_msg: String) -> Self {
        Self {
            failures_before_success: AtomicU32::new(999),
            error_to_return: Some(error_msg),
            download_attempts: Mutex::new(Vec::new()),
            successful_downloads: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl AudioDownloader for MockAudioDownloader {
    async fn stream_track(
        &self,
        _file_id: &FileId,
        _track_uri: &SpotifyUri,
        cache_path: &str,
    ) -> Result<()> {
        // Record attempt
        self.download_attempts
            .lock()
            .unwrap()
            .push(cache_path.to_string());

        // Check if we should still fail
        let remaining_failures = self.failures_before_success.load(Ordering::SeqCst);

        if remaining_failures > 0 {
            self.failures_before_success.fetch_sub(1, Ordering::SeqCst);

            return Err(anyhow!(
                "{}",
                self.error_to_return
                    .as_ref()
                    .unwrap_or(&"Streaming failed".to_string())
            ));
        }

        // Success - write fake audio data
        std::fs::write(cache_path, b"fake ogg vorbis audio data")?;

        self.successful_downloads
            .lock()
            .unwrap()
            .push(cache_path.to_string());

        Ok(())
    }
}

#[derive(Debug)]
pub struct MockTrackMetadataProvider {
    pub id: String,
    pub name: String,
    pub album_id: String,
    pub album_name: String,
    pub artist_names: Vec<String>,
    pub duration_ms: u32,
    pub year: i32,
    pub track_number: u32,
    pub files: HashMap<AudioFileFormat, FileId>,
}

#[async_trait]
impl TrackMetadataProvider for MockTrackMetadataProvider {
    async fn id(&self) -> String {
        self.id.clone()
    }
    async fn name(&self) -> String {
        self.name.clone()
    }
    async fn album_id(&self) -> String {
        self.album_id.clone()
    }
    async fn album_name(&self) -> String {
        self.album_name.clone()
    }
    async fn artist_names(&self) -> Vec<String> {
        self.artist_names.clone()
    }
    async fn duration_ms(&self) -> u32 {
        self.duration_ms
    }
    async fn year(&self) -> i32 {
        self.year
    }
    async fn track_number(&self) -> u32 {
        self.track_number
    }
    async fn get_file_id(&self, format: &librespot_metadata::audio::AudioFileFormat) -> Option<FileId> {
        self.files.get(format).copied()
    }
}
