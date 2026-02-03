//! Trait abstractions for testability.
//!
//! Defines the `AudioDownloader` trait to decouple streaming logic from
//! librespot implementation, enabling mocking in tests.

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
