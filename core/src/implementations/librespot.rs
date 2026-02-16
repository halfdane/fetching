//! Librespot-based implementations of traits.

use anyhow::Result;
use async_trait::async_trait;
use librespot_core::file_id::FileId;
use librespot_core::SpotifyUri;
use librespot_metadata::Metadata;
use reqwest;

/// Real implementation using librespot Session
pub struct LibrespotImageDownloader<'a> {
    pub session: &'a librespot_core::Session,
}

#[async_trait]
impl crate::traits::ImageDownloader for LibrespotImageDownloader<'_> {
    async fn download_cover(&self, file_id: &FileId) -> Result<Vec<u8>> {
        let image_bytes = self.session.spclient().get_image(file_id).await?;
        Ok(image_bytes.to_vec())
    }
}

/// Real implementation using librespot Session
pub struct LibrespotTrackFetcher<'a> {
    pub session: &'a librespot_core::Session,
}

#[async_trait]
impl crate::traits::TrackFetcher for LibrespotTrackFetcher<'_> {
    async fn fetch_track(&self, uri: &SpotifyUri) -> Result<librespot_metadata::track::Track> {
        librespot_metadata::track::Track::get(self.session, uri).await.map_err(anyhow::Error::from)
    }
}

/// Librespot implementation for fetching album metadata
pub struct LibrespotAlbumFetcher<'a> {
    pub session: &'a librespot_core::session::Session,
}

#[async_trait]
impl crate::traits::AlbumFetcher for LibrespotAlbumFetcher<'_> {
    async fn fetch_album(&self, uri: &librespot_core::SpotifyUri) -> Result<Box<dyn crate::traits::metadata::AlbumMetadataProvider>> {
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
impl crate::traits::metadata::AlbumMetadataProvider for LibrespotAlbumProvider {
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

/// Librespot implementation for fetching playlist metadata
pub struct LibrespotPlaylistFetcher<'a> {
    pub session: &'a librespot_core::session::Session,
}

impl<'a> std::fmt::Debug for LibrespotPlaylistFetcher<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibrespotPlaylistFetcher")
            .field("session", &"<Session>")
            .finish()
    }
}

#[async_trait]
impl crate::traits::PlaylistFetcher for LibrespotPlaylistFetcher<'_> {
    async fn fetch_playlist(&self, uri: &librespot_core::SpotifyUri) -> Result<Box<dyn crate::traits::metadata::PlaylistMetadataProvider>> {
        let playlist = librespot_metadata::playlist::Playlist::get(self.session, uri).await?;
        Ok(Box::new(LibrespotPlaylistProvider { playlist }))
    }
}

/// Wrapper to implement PlaylistMetadataProvider for librespot Playlist
#[derive(Debug)]
pub struct LibrespotPlaylistProvider {
    pub playlist: librespot_metadata::playlist::Playlist,
}

#[async_trait]
impl crate::traits::metadata::PlaylistMetadataProvider for LibrespotPlaylistProvider {
    async fn playlist_name(&self) -> String {
        self.playlist.name().to_string()
    }

    async fn playlist_tracks(&self) -> Vec<librespot_core::SpotifyUri> {
        self.playlist.tracks().cloned().collect()
    }

    async fn playlist_cover_art_bytes(&self) -> Option<Vec<u8>> {
        // Check if playlist has embedded cover art
        if !self.playlist.attributes.picture.is_empty() {
            return Some(self.playlist.attributes.picture.clone());
        }

        // Try to fetch from picture_sizes URLs
        if let Some(picture_size) = self.playlist.attributes.picture_sizes.first() {
            match reqwest::get(&picture_size.url).await {
                Ok(response) => match response.bytes().await {
                    Ok(bytes) => return Some(bytes.to_vec()),
                    Err(_) => {}
                },
                Err(_) => {}
            }
        }

        None
    }
}