use std::sync::Arc;

use async_trait::async_trait;
use librespot_core::session::Session;
use librespot_core::{FileId, SpotifyUri};
use librespot_metadata::{Metadata};
use moka::future::Cache;

use crate::container::{TrackCollection, Track};
use crate::spotify_api::{SpotifyCollectionMetadata, SpotifyCover, SpotifyTrackMetadata};

pub struct LibrespotTrackMetadataFetcher {
    session: Session,
}

impl LibrespotTrackMetadataFetcher {
    pub async fn new(session: &Session) -> anyhow::Result<Self> {
        Ok(Self { session: session.clone() })
    }
}

impl SpotifyTrackMetadata for LibrespotTrackMetadataFetcher {

    fn fetch_single_episode(&self, spotify_uri: &SpotifyUri) -> Result<(Track, String), anyhow::Error> {
        let l_episode = futures::executor::block_on(
            librespot_metadata::episode::Episode::get(&self.session, spotify_uri))?;
        
        let cover_id = l_episode.covers.first().map(|c| c.id.to_string()).unwrap_or_default();

        let track = Track { 
            spotify_id: spotify_uri.to_id()?,
            uri_str: spotify_uri.to_string(),
            title: l_episode.name, 
            artists: vec![], 
            duration_ms: l_episode.duration, 
            cover_id: Some(cover_id.clone()),
            explicit: l_episode.is_explicit, 
            language: vec![], 
            isrc: None,

            date: l_episode.publish_time.to_string(),
            popularity: None,
            disc_number: None,
            number: l_episode.number,
        };
        Ok((track, cover_id))
     }
    
    fn fetch_single_track(&self, spotify_uri: &SpotifyUri) -> Result<(Track, String), anyhow::Error> {
        let l_track = futures::executor::block_on(
            librespot_metadata::track::Track::get(&self.session, spotify_uri))?;
        
        tracing::info!("Fetched track metadata: {}", l_track.name);

        let cover_id = l_track.album.covers.first().map(|c| c.id.to_string()).unwrap_or_default();
        let track: Track = Track { 
                    spotify_id: spotify_uri.to_id()?,
                    uri_str: spotify_uri.to_string(),
                    title: l_track.name, 
                    artists: l_track.artists.iter().map(|a| a.name.clone()).collect(), 
                    duration_ms: l_track.duration, 
                    cover_id: Some(cover_id.clone()),
                    explicit: l_track.is_explicit, 
                    language: l_track.language_of_performance, 
                    isrc: l_track.external_ids.iter().find(|id| id.external_type == "isrc").map(|id| id.id.clone()),

                    date: l_track.album.date.to_string(),
                    popularity: Some(l_track.popularity),
                    disc_number: Some(l_track.disc_number),
                    number: l_track.number,
                };
        Ok((track, cover_id))
    }
}

pub struct LibrespotCollectionMetadataFetcher<'a, T: SpotifyTrackMetadata> {
    session: Session,
    track_fetcher: &'a T,
}

impl<'a, T: SpotifyTrackMetadata> LibrespotCollectionMetadataFetcher<'a, T> {
    pub async fn new(session: &Session, track_fetcher: &'a T) -> anyhow::Result<Self> {
        Ok(Self { session: session.clone(), track_fetcher })
    }
}

impl<'a, T: SpotifyTrackMetadata> SpotifyCollectionMetadata for LibrespotCollectionMetadataFetcher<'a, T> {

    fn fetch_album(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_album = futures::executor::block_on(
            librespot_metadata::album::Album::get(&self.session, spotify_uri))?;

        tracing::info!("Fetched album metadata: {}", l_album.name);

        let track_uris = l_album.tracks()
            .map(|uri| uri.to_string())
            .collect::<Vec<_>>();

        let cover_id = l_album.covers.first().map(|c| c.id.to_string()).unwrap_or_default();

        Ok(TrackCollection { 
            uri_str: l_album.id.to_string(), 
            spotify_id: l_album.id.to_id()?, 
            title: l_album.name, 
            artists: l_album.artists.iter().map(|a| a.name.clone()).collect(),
            total_tracks: track_uris.len(),
            cover_id: Some(cover_id.clone()), 
            track_uris: track_uris, 
            upc: l_album.external_ids.iter().find(|id| id.external_type == "upc").map(|id| id.id.clone()),
            popularity: Some(l_album.popularity),
            label: Some(l_album.label),
            date: Some(l_album.date.to_string()),
        })
    }

    fn fetch_track(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_album = futures::executor::block_on(
            librespot_metadata::album::Album::get(&self.session, spotify_uri))?;

        let (track, cover_id) = self.track_fetcher.fetch_single_track(spotify_uri)?;
        Ok(TrackCollection { 
            uri_str: l_album.id.to_string(), 
            spotify_id: l_album.id.to_id()?, 
            title: l_album.name, 
            artists: track.artists.clone(),
            total_tracks: 1, 
            cover_id: Some(cover_id.clone()), 
            track_uris: vec![track.uri_str.clone()], 
            upc: l_album.external_ids.iter().find(|id| id.external_type == "upc").map(|id| id.id.clone()),
            popularity: Some(l_album.popularity),
            label: Some(l_album.label),
            date: Some(l_album.date.to_string()),
        })
    }

    fn fetch_playlist(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_playlist = futures::executor::block_on(
            librespot_metadata::playlist::Playlist::get(&self.session, spotify_uri))?;

        tracing::info!("Fetched playlist metadata: {}", l_playlist.attributes.name);

        let track_uris = l_playlist.tracks()
            .map(|uri| uri.to_string())
            .collect::<Vec<_>>();
        Ok(TrackCollection { 
            uri_str: l_playlist.id.to_string(), 
            spotify_id: l_playlist.id.to_id()?, 
            title: l_playlist.attributes.name.clone(), 
            artists: vec![],
            total_tracks: track_uris.len(), 
            cover_id: None, 
            track_uris: track_uris, 
            upc: None,
            popularity: None,
            label: None,
            date: None,
        })
    }

    fn fetch_show(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_show = futures::executor::block_on(
            librespot_metadata::show::Show::get(&self.session, spotify_uri))?;
        
        tracing::info!("Fetched show metadata: {}", l_show.name);

        let track_uris = l_show.episodes.iter()
            .map(|uri| uri.to_string())
            .collect::<Vec<_>>();

        let cover_id = l_show.covers.first().map(|c| c.id.to_string()).unwrap_or_default();

        Ok(TrackCollection { 
            uri_str: l_show.id.to_string(), 
            spotify_id: l_show.id.to_id()?, 
            title: l_show.name.clone(), 
            artists: vec![],
            total_tracks: track_uris.len(), 
            cover_id: Some(cover_id), 
            track_uris: track_uris, 
            upc: None,
            popularity: None,
            label: Some(l_show.publisher.clone()),
            date: None,
        })
    }

    fn fetch_episode(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let (track, cover_id) = self.track_fetcher.fetch_single_track(spotify_uri)?;
        Ok(TrackCollection { 
            uri_str: track.uri_str.clone(), 
            spotify_id: track.spotify_id, 
            title: track.title.clone(), 
            artists: track.artists.clone(),
            total_tracks: 1, 
            cover_id: Some(cover_id.clone()), 
            track_uris: vec![track.uri_str.clone()], 
            upc: None,
            popularity: None,
            label: None,
            date: None,
        })
    }

}


#[derive(Clone)]
pub struct LibrespotCoverFetcher {
    session: Session,
    cover_cache: Arc<Cache<String, Vec<u8>>>
}

impl LibrespotCoverFetcher {
    pub async fn new(session: &Session) -> anyhow::Result<Self> {
        let cover_cache: Arc<Cache<String, Vec<u8>>> = Arc::new(Cache::new(1000));
        Ok(Self { session: session.clone(), cover_cache })
    }
}


#[async_trait]
impl SpotifyCover for LibrespotCoverFetcher {
    async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>> {
        match self.cover_cache.try_get_with::<_, anyhow::Error>(cover_id.to_string(), async move {
            let bytes = hex::decode(&cover_id).map_err(|e| anyhow::Error::msg(e.to_string()))?;
            let file_id = FileId::from_raw(&bytes);
            let image_bytes = self.session
                .spclient()
                .get_image(&file_id)
                .await
                .map_err(|e| anyhow::Error::msg(e.to_string()))?;
            Ok(image_bytes.to_vec())
        }).await {
            Ok(cover_data) => Ok(cover_data),
            Err(e) => Err(anyhow::Error::msg(format!("Failed to fetch cover {}: {}", cover_id, e))),
        }
    }
}


