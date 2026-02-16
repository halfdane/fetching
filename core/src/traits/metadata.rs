//! Metadata provider traits for accessing track, album, and playlist information.

use async_trait::async_trait;
use librespot_core::file_id::FileId;
use librespot_metadata::audio::AudioFileFormat;
use std::fmt::Debug;

/// Abstracts track metadata access for testability
#[async_trait]
pub trait TrackMetadataProvider: Send + Sync + Debug {
    async fn name(&self) -> String;
    async fn album_id(&self) -> String;
    async fn album_name(&self) -> String;
    async fn artist_names(&self) -> Vec<String>;
    async fn album_artist_names(&self) -> Vec<String>;
    async fn duration_ms(&self) -> u32;
    async fn date(&self) -> Option<String>; // Formatted date: "YYYY-MM-DD", "YYYY", or None
    async fn track_number(&self) -> u32;
    async fn disc_number(&self) -> u32;
    async fn genres(&self) -> Vec<String>;
    async fn isrc(&self) -> Option<String>;
    async fn label(&self) -> Option<String>;
    async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId>;

    // Album cover information for testability
    async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId>;

    // Alternative track URIs for different audio formats
    async fn alternative_uris(&self) -> Vec<String>;
}

/// Provides access to album metadata in a testable way
#[async_trait]
pub trait AlbumMetadataProvider: Send + Sync + Debug {
    /// Get the album name
    async fn album_name(&self) -> String;

    /// Get the album artists
    async fn album_artists(&self) -> Vec<String>;

    /// Get the album cover file IDs (for downloading cover art)
    async fn album_cover_file_ids(&self) -> Vec<librespot_core::FileId>;

    /// Get the track URIs in this album
    async fn album_track_uris(&self) -> Vec<librespot_core::SpotifyUri>;
}

/// Provides metadata for a playlist
#[async_trait]
pub trait PlaylistMetadataProvider: Send + Sync + Debug {
    async fn playlist_name(&self) -> String;
    async fn playlist_tracks(&self) -> Vec<librespot_core::SpotifyUri>;
    async fn playlist_cover_art_bytes(&self) -> Option<Vec<u8>>;
}