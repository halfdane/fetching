//! Trait abstractions for testability.
//!
//! Defines the `AudioDownloader` trait to decouple streaming logic from
//! librespot implementation, enabling mocking in tests.

use anyhow::Result;
use async_trait::async_trait;
use librespot_core::file_id::FileId;
use librespot_core::SpotifyUri;
use librespot_metadata::audio::AudioFileFormat;

/// Capability to stream and cache audio tracks
#[async_trait]
pub trait AudioDownloader: Send + Sync {
    /// Stream and cache a track's audio file to the specified path
    async fn stream_track(
        &self,
        file_id: &FileId,
        track_uri: &SpotifyUri,
        cache_path: &str,
    ) -> Result<()>;
}

use std::fmt::Debug;

/// Abstracts track metadata access for testability
#[async_trait]
pub trait TrackMetadataProvider: Send + Sync + Debug {
    async fn id(&self) -> String;
    async fn name(&self) -> String;
    async fn album_id(&self) -> String;
    async fn album_name(&self) -> String;
    async fn artist_names(&self) -> Vec<String>;
    async fn duration_ms(&self) -> u32;
    async fn year(&self) -> i32;
    async fn track_number(&self) -> u32;
    async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId>;
}

/// Real implementation for librespot_metadata::track::Track
#[derive(Debug)]
pub struct LibrespotTrackProvider<'a> {
    pub track: &'a librespot_metadata::track::Track,
}

#[async_trait]
impl<'a> TrackMetadataProvider for LibrespotTrackProvider<'a> {
    async fn id(&self) -> String {
        self.track.id.to_string()
    }
    async fn name(&self) -> String {
        self.track.name.clone()
    }
    async fn album_id(&self) -> String {
        self.track.album.id.to_string()
    }
    async fn album_name(&self) -> String {
        self.track.album.name.clone()
    }
    async fn artist_names(&self) -> Vec<String> {
        self.track.artists.iter().map(|a| a.name.clone()).collect()
    }
    async fn duration_ms(&self) -> u32 {
        self.track.duration as u32
    }
    async fn year(&self) -> i32 {
        self.track.album.date.year()
    }
    async fn track_number(&self) -> u32 {
        self.track.number as u32
    }
    async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId> {
        self.track.files.get(format).copied()
    }
}
