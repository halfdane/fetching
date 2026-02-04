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
    async fn date(&self) -> Option<String>; // Formatted date: "YYYY-MM-DD", "YYYY", or None
    async fn track_number(&self) -> u32;
    async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId>;
    
    // Album cover information for testability
    async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId>;
    
    // Alternative track URIs for different audio formats
    async fn alternative_uris(&self) -> Vec<String>;
}

/// Real implementation for librespot_metadata::track::Track
#[derive(Debug)]
pub struct LibrespotTrackProvider<'a> {
    pub track: &'a librespot_metadata::track::Track,
}

/// Capability to download cover images
#[async_trait]
pub trait ImageDownloader: Send + Sync {
    /// Download a cover image by its Spotify file ID
    async fn download_cover(&self, file_id: &FileId) -> Result<Vec<u8>>;
    
    /// Download an image from a direct URL
    async fn download_url(&self, url: &str) -> Result<Vec<u8>>;
}

/// Real implementation using librespot Session
pub struct LibrespotImageDownloader<'a> {
    pub session: &'a librespot_core::Session,
}

#[async_trait]
impl<'a> ImageDownloader for LibrespotImageDownloader<'a> {
    async fn download_cover(&self, file_id: &FileId) -> Result<Vec<u8>> {
        let image_bytes = self.session.spclient().get_image(file_id).await?;
        Ok(image_bytes.to_vec())
    }
    
    async fn download_url(&self, url: &str) -> Result<Vec<u8>> {
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}

/// Mock implementation for testing
#[derive(Debug, Default)]
pub struct MockImageDownloader {
    pub cover_images: std::collections::HashMap<FileId, Vec<u8>>,
    pub url_images: std::collections::HashMap<String, Vec<u8>>,
}

#[async_trait]
impl ImageDownloader for MockImageDownloader {
    async fn download_cover(&self, file_id: &FileId) -> Result<Vec<u8>> {
        self.cover_images.get(file_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Cover image not found for FileId"))
    }
    
    async fn download_url(&self, url: &str) -> Result<Vec<u8>> {
        self.url_images.get(url)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("URL image not found: {}", url))
    }
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
    async fn date(&self) -> Option<String> {
        let date_obj = self.track.album.date;
        let year = date_obj.year();
        let month = date_obj.month() as u8;
        let day = date_obj.day();
        
        if year > 0 && month > 0 && day > 0 {
            Some(format!("{:04}-{:02}-{:02}", year, month, day))
        } else if year > 0 {
            Some(year.to_string())
        } else {
            None
        }
    }
    async fn track_number(&self) -> u32 {
        self.track.number as u32
    }
    async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId> {
        self.track.files.get(format).copied()
    }
    
    async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
        self.track.album.covers.get(index).map(|cover| cover.id)
    }
    
    async fn alternative_uris(&self) -> Vec<String> {
        self.track.alternatives.iter().map(|uri| uri.to_string()).collect()
    }
}
