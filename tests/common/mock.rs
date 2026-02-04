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

        // Success - write valid minimal OGG Vorbis data
        use std::fs;
        let mut writer = ogg::PacketWriter::new(fs::File::create(cache_path).unwrap());

        // Vorbis identification header (minimal valid header)
        let ident_header = vec![
            0x01, // packet type (identification)
            0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, // "vorbis"
            0x00, 0x00, 0x00, 0x00, // version
            0x02, // channels
            0x44, 0xac, 0x00, 0x00, // sample rate (44100)
            0x00, 0x00, 0x00, 0x00, // max bitrate
            0x00, 0x7d, 0x00, 0x00, // nominal bitrate (32000)
            0x00, 0x00, 0x00, 0x00, // min bitrate
            0xb8, // blocksize
            0x01, // framing flag
        ];

        // Vorbis comment header (empty)
        let comment_header = vec![
            0x03, // packet type (comments)
            0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, // "vorbis"
            0x00, 0x00, 0x00, 0x00, // vendor length (0)
            0x00, 0x00, 0x00, 0x00, // comment count (0)
            0x01, // framing flag
        ];

        // Setup header (minimal)
        let setup_header = vec![
            0x05, // packet type (setup)
            0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, // "vorbis"
            0x01, // framing flag
        ];

        writer
            .write_packet(ident_header, 0, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        writer
            .write_packet(comment_header, 0, ogg::PacketWriteEndInfo::NormalPacket, 0)
            .unwrap();
        writer
            .write_packet(setup_header, 0, ogg::PacketWriteEndInfo::EndStream, 0)
            .unwrap();
        drop(writer);

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
    pub album_artist_names: Vec<String>,
    pub duration_ms: u32,
    pub year: i32,
    pub track_number: u32,
    pub disc_number: u32,
    pub genres: Vec<String>,
    pub isrc: Option<String>,
    pub label: Option<String>,
    pub files: HashMap<AudioFileFormat, FileId>,
    pub album_cover_file_ids: Vec<FileId>,
    pub alternative_uris: Vec<String>,
}

#[async_trait]
impl TrackMetadataProvider for MockTrackMetadataProvider {
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
    async fn get_file_id(&self, format: &librespot_metadata::audio::AudioFileFormat) -> Option<FileId> {
        self.files.get(format).copied()
    }
    
    async fn album_artist_names(&self) -> Vec<String> {
        self.album_artist_names.clone()
    }
    async fn disc_number(&self) -> u32 {
        self.disc_number
    }
    async fn genres(&self) -> Vec<String> {
        self.genres.clone()
    }
    async fn isrc(&self) -> Option<String> {
        self.isrc.clone()
    }
    async fn label(&self) -> Option<String> {
        self.label.clone()
    }
    
    async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
        self.album_cover_file_ids.get(index).copied()
    }
    
    async fn alternative_uris(&self) -> Vec<String> {
        self.alternative_uris.clone()
    }
}
