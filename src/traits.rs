//! Trait abstractions for testability.
//!
//! Defines the `AudioDownloader` trait to decouple streaming logic from
//! librespot implementation, enabling mocking in tests.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use librespot_core::file_id::FileId;
use librespot_core::SpotifyUri;
use librespot_metadata::audio::AudioFileFormat;
use librespot_metadata::Metadata;

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

/// Real implementation for librespot_metadata::track::Track
#[derive(Debug)]
pub struct OwnedLibrespotTrackProvider {
    pub track: librespot_metadata::track::Track,
}

/// Capability to download cover images
#[async_trait]
pub trait ImageDownloader: Send + Sync {
    /// Download a cover image by its Spotify file ID
    async fn download_cover(&self, file_id: &FileId) -> Result<Vec<u8>>;
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
}

/// Capability to fetch track metadata
#[async_trait]
pub trait TrackFetcher: Send + Sync {
    async fn fetch_track(&self, uri: &SpotifyUri) -> Result<librespot_metadata::track::Track>;
}

/// Real implementation using librespot Session
pub struct LibrespotTrackFetcher<'a> {
    pub session: &'a librespot_core::Session,
}

#[async_trait]
impl<'a> TrackFetcher for LibrespotTrackFetcher<'a> {
    async fn fetch_track(&self, uri: &SpotifyUri) -> Result<librespot_metadata::track::Track> {
        librespot_metadata::track::Track::get(self.session, uri).await.map_err(anyhow::Error::from)
    }
}

/// Mock implementation for testing
#[derive(Debug, Default)]
pub struct MockImageDownloader {
    pub cover_images: std::collections::HashMap<FileId, Vec<u8>>,
}

#[async_trait]
impl ImageDownloader for MockImageDownloader {
    async fn download_cover(&self, file_id: &FileId) -> Result<Vec<u8>> {
        self.cover_images.get(file_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Cover image not found for FileId"))
    }
}

/// Mock implementation for testing audio downloading
#[derive(Debug, Default)]
pub struct MockAudioDownloader {
    pub audio_files: std::collections::HashMap<FileId, Vec<u8>>,
}

#[async_trait]
impl AudioDownloader for MockAudioDownloader {
    async fn stream_track(
        &self,
        file_id: &FileId,
        _track_uri: &SpotifyUri,
        cache_path: &str,
    ) -> Result<()> {
        if let Some(audio_data) = self.audio_files.get(file_id) {
            // If the data is the fake string, create valid OGG data instead
            if audio_data == b"fake ogg vorbis audio data" || audio_data == b"fake ogg audio data" {
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
            } else {
                std::fs::write(cache_path, audio_data)?;
            }
            Ok(())
        } else {
            Err(anyhow!("Audio file not found for FileId"))
        }
    }
}

#[async_trait]
impl TrackMetadataProvider for OwnedLibrespotTrackProvider {
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
    
    async fn album_artist_names(&self) -> Vec<String> {
        self.track.album.artists.iter().map(|a| a.name.clone()).collect()
    }
    async fn disc_number(&self) -> u32 {
        self.track.disc_number as u32
    }
    async fn genres(&self) -> Vec<String> {
        self.track.tags.clone()
    }
    async fn isrc(&self) -> Option<String> {
        self.track
            .external_ids
            .iter()
            .find(|eid| eid.external_type == "isrc")
            .map(|eid| eid.id.clone())
    }
    async fn label(&self) -> Option<String> {
        if !self.track.album.label.is_empty() {
            Some(self.track.album.label.clone())
        } else {
            None
        }
    }
    
    async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
        self.track.album.covers.get(index).map(|cover| cover.id)
    }
    
    async fn alternative_uris(&self) -> Vec<String> {
        self.track.alternatives.iter().map(|uri| uri.to_string()).collect()
    }
}

/// Mock implementation for testing track fetching
#[derive(Debug, Default)]
pub struct MockTrackFetcher {
    pub tracks: std::collections::HashMap<String, librespot_metadata::track::Track>,
}

#[async_trait]
impl TrackFetcher for MockTrackFetcher {
    async fn fetch_track(&self, uri: &SpotifyUri) -> Result<librespot_metadata::track::Track> {
        let uri_str = uri.to_string();
        self.tracks.get(&uri_str)
            .cloned()
            .ok_or_else(|| anyhow!("Track not found: {}", uri_str))
    }
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

/// Fetches album metadata from Spotify
#[async_trait]
pub trait AlbumFetcher: Send + Sync {
    /// Fetch album metadata by URI
    async fn fetch_album(&self, uri: &librespot_core::SpotifyUri) -> Result<Box<dyn AlbumMetadataProvider>>;
}

/// Librespot implementation for fetching album metadata
pub struct LibrespotAlbumFetcher<'a> {
    pub session: &'a librespot_core::session::Session,
}

#[async_trait]
impl<'a> AlbumFetcher for LibrespotAlbumFetcher<'a> {
    async fn fetch_album(&self, uri: &librespot_core::SpotifyUri) -> Result<Box<dyn AlbumMetadataProvider>> {
        let album = librespot_metadata::album::Album::get(self.session, uri).await?;
        Ok(Box::new(LibrespotAlbumProvider { album }))
    }
}

/// Wrapper to implement AlbumMetadataProvider for librespot Album
#[derive(Debug)]
pub struct LibrespotAlbumProvider {
    pub album: librespot_metadata::album::Album,
}

#[async_trait]
impl AlbumMetadataProvider for LibrespotAlbumProvider {
    async fn album_name(&self) -> String {
        self.album.name.clone()
    }

    async fn album_artists(&self) -> Vec<String> {
        self.album.artists.iter().map(|a| a.name.clone()).collect()
    }

    async fn album_cover_file_ids(&self) -> Vec<librespot_core::FileId> {
        self.album.covers.iter().map(|cover| cover.id).collect()
    }

    async fn album_track_uris(&self) -> Vec<librespot_core::SpotifyUri> {
        self.album.tracks().cloned().collect()
    }
}

/// Mock implementation for testing album fetching
pub struct MockAlbumFetcher {
    pub albums: std::collections::HashMap<String, std::sync::Arc<dyn AlbumMetadataProvider>>,
}

impl Default for MockAlbumFetcher {
    fn default() -> Self {
        Self {
            albums: std::collections::HashMap::new(),
        }
    }
}

impl MockAlbumFetcher {
    /// Add an album to the mock
    pub fn add_album<P: AlbumMetadataProvider + 'static>(&mut self, uri: &str, album: P) {
        self.albums.insert(uri.to_string(), std::sync::Arc::new(album));
    }
}

#[async_trait]
impl AlbumFetcher for MockAlbumFetcher {
    async fn fetch_album(&self, uri: &librespot_core::SpotifyUri) -> Result<Box<dyn AlbumMetadataProvider>> {
        let uri_str = uri.to_string();
        self.albums.get(&uri_str)
            .map(|album| Box::new(ArcAlbumProvider(album.clone())) as Box<dyn AlbumMetadataProvider>)
            .ok_or_else(|| anyhow!("Album not found: {}", uri_str))
    }
}

/// Wrapper to make Arc<dyn AlbumMetadataProvider> implement AlbumMetadataProvider
#[derive(Clone, Debug)]
struct ArcAlbumProvider(std::sync::Arc<dyn AlbumMetadataProvider>);

#[async_trait]
impl AlbumMetadataProvider for ArcAlbumProvider {
    async fn album_name(&self) -> String {
        self.0.album_name().await
    }

    async fn album_artists(&self) -> Vec<String> {
        self.0.album_artists().await
    }

    async fn album_cover_file_ids(&self) -> Vec<librespot_core::FileId> {
        self.0.album_cover_file_ids().await
    }

    async fn album_track_uris(&self) -> Vec<librespot_core::SpotifyUri> {
        self.0.album_track_uris().await
    }
}

/// Mock album metadata for testing
#[derive(Debug, Clone)]
pub struct MockAlbumMetadata {
    pub name: String,
    pub artists: Vec<String>,
    pub cover_file_ids: Vec<librespot_core::FileId>,
    pub track_uris: Vec<librespot_core::SpotifyUri>,
}

#[async_trait]
impl AlbumMetadataProvider for MockAlbumMetadata {
    async fn album_name(&self) -> String {
        self.name.clone()
    }

    async fn album_artists(&self) -> Vec<String> {
        self.artists.clone()
    }

    async fn album_cover_file_ids(&self) -> Vec<librespot_core::FileId> {
        self.cover_file_ids.clone()
    }

    async fn album_track_uris(&self) -> Vec<librespot_core::SpotifyUri> {
        self.track_uris.clone()
    }
}

/// Provides metadata for a playlist
#[async_trait]
pub trait PlaylistMetadataProvider: Send + Sync + Debug {
    async fn playlist_name(&self) -> String;
    async fn playlist_tracks(&self) -> Vec<librespot_core::SpotifyUri>;
    async fn playlist_cover_art_bytes(&self) -> Option<Vec<u8>>;
}

/// Fetches playlist metadata from Spotify
#[async_trait]
pub trait PlaylistFetcher: Send + Sync + Debug {
    async fn fetch_playlist(&self, uri: &librespot_core::SpotifyUri) -> anyhow::Result<Box<dyn PlaylistMetadataProvider>>;
}

/// Mock playlist metadata for testing
#[derive(Debug, Clone)]
pub struct MockPlaylistMetadata {
    pub name: String,
    pub track_uris: Vec<librespot_core::SpotifyUri>,
    pub cover_art_bytes: Option<Vec<u8>>,
}

#[async_trait]
impl PlaylistMetadataProvider for MockPlaylistMetadata {
    async fn playlist_name(&self) -> String {
        self.name.clone()
    }

    async fn playlist_tracks(&self) -> Vec<librespot_core::SpotifyUri> {
        self.track_uris.clone()
    }

    async fn playlist_cover_art_bytes(&self) -> Option<Vec<u8>> {
        self.cover_art_bytes.clone()
    }
}

/// Mock playlist fetcher for testing
#[derive(Debug)]
pub struct MockPlaylistFetcher {
    playlists: std::collections::HashMap<String, MockPlaylistMetadata>,
}

impl Default for MockPlaylistFetcher {
    fn default() -> Self {
        Self {
            playlists: std::collections::HashMap::new(),
        }
    }
}

impl MockPlaylistFetcher {
    /// Add a playlist to the mock
    pub fn add_playlist(&mut self, uri: &str, playlist: MockPlaylistMetadata) {
        self.playlists.insert(uri.to_string(), playlist);
    }
}

#[async_trait]
impl PlaylistFetcher for MockPlaylistFetcher {
    async fn fetch_playlist(&self, uri: &librespot_core::SpotifyUri) -> anyhow::Result<Box<dyn PlaylistMetadataProvider>> {
        let uri_str = uri.to_string();
        match self.playlists.get(&uri_str) {
            Some(playlist) => Ok(Box::new(playlist.clone())),
            None => anyhow::bail!("Playlist not found: {}", uri_str),
        }
    }
}
