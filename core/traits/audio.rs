//! Audio-related traits for downloading and streaming.

use anyhow::Result;
use async_trait::async_trait;
use librespot_core::file_id::FileId;
use librespot_core::SpotifyUri;

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

/// Capability to download cover images
#[async_trait]
pub trait ImageDownloader: Send + Sync {
    /// Download a cover image by its Spotify file ID
    async fn download_cover(&self, file_id: &FileId) -> Result<Vec<u8>>;
}