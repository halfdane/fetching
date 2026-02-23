use librespot_core::SpotifyUri;

use crate::container::{TrackCollection};

pub trait SpotifyMetadata {
    fn fetch_album(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_playlist(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_track(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_episode(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_show(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
}

// Collection factory - dispatch + constructors
pub fn fetch_collection(uri_str: &str, fetcher: &impl SpotifyMetadata) -> anyhow::Result<TrackCollection> {
    let spotify_uri = &SpotifyUri::from_uri(uri_str)?;
    
    let collection = match spotify_uri.item_type() {
        "album" => fetcher.fetch_album(spotify_uri)?,
        "track" => fetcher.fetch_track(spotify_uri)?,
        "playlist" => fetcher.fetch_playlist(spotify_uri)?,
        "show" => fetcher.fetch_show(spotify_uri)?,
        "episode" => fetcher.fetch_episode(spotify_uri)?,
        _ => anyhow::bail!("Unsupported URI type: {}", uri_str),
    };
    
    Ok(collection)
}
