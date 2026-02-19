//! Fetcher traits for retrieving metadata from Spotify.

use anyhow::Result;
use async_trait::async_trait;
use librespot_core::SpotifyUri;

/// Capability to fetch track metadata
#[async_trait]
pub trait TrackFetcher: Send + Sync {
    async fn fetch_track(&self, uri: &SpotifyUri) -> Result<librespot_metadata::track::Track>;
}

/// Fetches album metadata from Spotify
#[async_trait]
pub trait AlbumFetcher: Send + Sync {
    /// Fetch album metadata by URI
    async fn fetch_album(
        &self,
        uri: &librespot_core::SpotifyUri,
    ) -> Result<Box<dyn crate::traits::metadata::AlbumMetadataProvider>>;
}

/// Fetches playlist metadata from Spotify
#[async_trait]
pub trait PlaylistFetcher: Send + Sync + std::fmt::Debug {
    async fn fetch_playlist(
        &self,
        uri: &librespot_core::SpotifyUri,
    ) -> anyhow::Result<Box<dyn crate::traits::metadata::PlaylistMetadataProvider>>;
}
