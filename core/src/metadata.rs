use librespot_core::SpotifyUri;

use crate::container::{TrackCollection};

pub trait SpotifyMetadata {
    // fn fetch_album(&self, uri: &SpotifyUri) -> anyhow::Result<AlbumMetadata>;
    // fn fetch_playlist(&self, uri: &SpotifyUri) -> anyhow::Result<PlaylistMetadata>;
    fn fetch_track(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    // fn fetch_episode(&self, uri: &SpotifyUri) -> anyhow::Result<EpisodeMetadata>;
    // fn fetch_show(&self, uri: &SpotifyUri) -> anyhow::Result<ShowMetadata>;
}
