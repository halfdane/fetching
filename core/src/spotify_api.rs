use async_trait::async_trait;
use librespot_core::SpotifyUri;

use crate::container::{Track, TrackCollection};

pub trait SpotifyCollectionMetadata {
    fn fetch_album(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_playlist(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_track(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_episode(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_show(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
}

pub trait SpotifyTrackMetadata {
    fn fetch_single_episode(&self, spotify_uri: &SpotifyUri) -> Result<(Track, String), anyhow::Error> ;
    fn fetch_single_track(&self, spotify_uri: &SpotifyUri) -> Result<(Track, String), anyhow::Error> ;

    fn fetch_by_uri(&self, uri_str: &str) -> anyhow::Result<(Track, String)> {
        let spotify_uri = &SpotifyUri::from_uri(uri_str)?;
        
        match spotify_uri.item_type() {
            "track" => Ok(self.fetch_single_track(spotify_uri)?),
            "episode" => Ok(self.fetch_single_episode(spotify_uri)?),
            _ => anyhow::bail!("Unsupported URI type: {}", uri_str),
        }
    }
}

#[async_trait]
pub trait SpotifyCover: Clone + Send + Sync {
    async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>>;
}

pub fn fetch_collection(uri_str: &str, fetcher: &impl SpotifyCollectionMetadata) -> anyhow::Result<TrackCollection> {
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



