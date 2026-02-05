//! Mock implementations for testing.
//!
//! All mock structs are marked with #[allow(dead_code)] to suppress warnings
//! since they are only used in tests.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use librespot_core::file_id::FileId;
use librespot_core::SpotifyUri;
use librespot_metadata::audio::AudioFileFormat;
use std::sync::Arc;

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct MockImageDownloader {
    pub cover_images: std::collections::HashMap<FileId, Vec<u8>>,
}

#[async_trait]
impl crate::traits::ImageDownloader for MockImageDownloader {
    async fn download_cover(&self, file_id: &FileId) -> anyhow::Result<Vec<u8>> {
        self.cover_images.get(file_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Cover image not found for FileId"))
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct MockAudioDownloader {
    pub audio_files: std::collections::HashMap<FileId, Vec<u8>>,
}

#[async_trait]
impl crate::traits::AudioDownloader for MockAudioDownloader {
    async fn stream_track(
        &self,
        file_id: &FileId,
        _track_uri: &SpotifyUri,
        cache_path: &str,
    ) -> anyhow::Result<()> {
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
            Err(anyhow::anyhow!("Audio file not found for FileId"))
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct MockTrackFetcher {
    pub tracks: std::collections::HashMap<String, librespot_metadata::track::Track>,
}

#[async_trait]
impl crate::traits::TrackFetcher for MockTrackFetcher {
    async fn fetch_track(&self, uri: &SpotifyUri) -> anyhow::Result<librespot_metadata::track::Track> {
        let uri_str = uri.to_string();
        self.tracks.get(&uri_str)
            .cloned()
            .ok_or_else(|| anyhow!("Track not found: {}", uri_str))
    }
}

#[allow(dead_code)]
pub struct MockAlbumFetcher {
    pub albums: std::collections::HashMap<String, Arc<dyn crate::traits::AlbumMetadataProvider>>,
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
    pub fn add_album<P: crate::traits::AlbumMetadataProvider + 'static>(&mut self, uri: &str, album: P) {
        self.albums.insert(uri.to_string(), Arc::new(album));
    }
}

#[async_trait]
impl crate::traits::AlbumFetcher for MockAlbumFetcher {
    async fn fetch_album(&self, uri: &SpotifyUri) -> anyhow::Result<Box<dyn crate::traits::AlbumMetadataProvider>> {
        let uri_str = uri.to_string();
        self.albums.get(&uri_str)
            .map(|album| Box::new(ArcAlbumProvider(album.clone())) as Box<dyn crate::traits::AlbumMetadataProvider>)
            .ok_or_else(|| anyhow!("Album not found: {}", uri_str))
    }
}

/// Wrapper to make Arc<dyn AlbumMetadataProvider> implement AlbumMetadataProvider
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct ArcAlbumProvider(Arc<dyn crate::traits::AlbumMetadataProvider>);

#[async_trait]
impl crate::traits::AlbumMetadataProvider for ArcAlbumProvider {
    async fn album_name(&self) -> String {
        self.0.album_name().await
    }

    async fn album_artists(&self) -> Vec<String> {
        self.0.album_artists().await
    }

    async fn album_cover_file_ids(&self) -> Vec<FileId> {
        self.0.album_cover_file_ids().await
    }

    async fn album_track_uris(&self) -> Vec<SpotifyUri> {
        self.0.album_track_uris().await
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MockAlbumMetadata {
    pub name: String,
    pub artists: Vec<String>,
    pub cover_file_ids: Vec<FileId>,
    pub track_uris: Vec<SpotifyUri>,
}

#[async_trait]
impl crate::traits::AlbumMetadataProvider for MockAlbumMetadata {
    async fn album_name(&self) -> String {
        self.name.clone()
    }

    async fn album_artists(&self) -> Vec<String> {
        self.artists.clone()
    }

    async fn album_cover_file_ids(&self) -> Vec<FileId> {
        self.cover_file_ids.clone()
    }

    async fn album_track_uris(&self) -> Vec<SpotifyUri> {
        self.track_uris.clone()
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MockPlaylistMetadata {
    pub name: String,
    pub track_uris: Vec<SpotifyUri>,
    pub cover_art_bytes: Option<Vec<u8>>,
}

#[async_trait]
impl crate::traits::PlaylistMetadataProvider for MockPlaylistMetadata {
    async fn playlist_name(&self) -> String {
        self.name.clone()
    }

    async fn playlist_tracks(&self) -> Vec<SpotifyUri> {
        self.track_uris.clone()
    }

    async fn playlist_cover_art_bytes(&self) -> Option<Vec<u8>> {
        self.cover_art_bytes.clone()
    }
}

#[allow(dead_code)]
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
impl crate::traits::PlaylistFetcher for MockPlaylistFetcher {
    async fn fetch_playlist(&self, uri: &SpotifyUri) -> anyhow::Result<Box<dyn crate::traits::PlaylistMetadataProvider>> {
        let uri_str = uri.to_string();
        match self.playlists.get(&uri_str) {
            Some(playlist) => Ok(Box::new(playlist.clone())),
            None => anyhow::bail!("Playlist not found: {}", uri_str),
        }
    }
}