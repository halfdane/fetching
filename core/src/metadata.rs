use librespot_core::SpotifyUri;

use crate::container::{TrackCollection};

pub trait SpotifyMetadata {
    fn fetch_album(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_playlist(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_track(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    // fn fetch_episode(&self, uri: &SpotifyUri) -> anyhow::Result<EpisodeMetadata>;
    // fn fetch_show(&self, uri: &SpotifyUri) -> anyhow::Result<ShowMetadata>;
}

// Collection factory - dispatch + constructors
pub fn fetch_collection(uri_str: &str, fetcher: &impl SpotifyMetadata) -> anyhow::Result<TrackCollection> {
    let spotify_uri = &SpotifyUri::from_uri(uri_str)?;
    
    let collection = match spotify_uri.item_type() {
        "album" => fetcher.fetch_album(spotify_uri)?,
        "track" => fetcher.fetch_track(spotify_uri)?,
        "playlist" => fetcher.fetch_playlist(spotify_uri)?,
        // "episode" => Container::new_episode(uri_str, fetcher)?,
        // "show" => Container::new_show(uri_str, fetcher)?,
        _ => anyhow::bail!("Unsupported URI type: {}", uri_str),
    };
    
    Ok(collection)
}
