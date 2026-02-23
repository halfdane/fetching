use librespot_core::session::Session;
use librespot_core::{SpotifyUri};
use librespot_metadata::{Metadata};

use crate::container::{TrackCollection, Track};
use crate::metadata::SpotifyMetadata;

pub struct LibrespotMetadataFetcher {
    session: Session,
}

impl LibrespotMetadataFetcher {
    pub async fn new(session: &Session) -> anyhow::Result<Self> {
        Ok(Self { session: session.clone() })
    }

    fn fetch_single_episode(&self, spotify_uri: &SpotifyUri) -> Result<(Track, String), anyhow::Error> {
        let l_episode = futures::executor::block_on(
            librespot_metadata::episode::Episode::get(&self.session, spotify_uri))?;
        
        let cover_id = l_episode.covers.first().map(|c| c.id.to_string()).unwrap_or_default();

        let mut track: Track = Track { 
            spotify_id: spotify_uri.to_id()?,
            uri_str: spotify_uri.to_string(),
            spotify_uri: spotify_uri.clone().into(), 
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
        Ok((track.rehydrate()?, cover_id))
     }
    
    fn fetch_single_track(&self, spotify_uri: &SpotifyUri) -> Result<(Track, String, librespot_metadata::Album), anyhow::Error> {
        let l_track = futures::executor::block_on(
            librespot_metadata::track::Track::get(&self.session, spotify_uri))?;
        
        tracing::info!("Fetched track metadata: {}", l_track.name);

        let cover_id = l_track.album.covers.first().map(|c| c.id.to_string()).unwrap_or_default();
        let track: Track = Track { 
                    spotify_id: spotify_uri.to_id()?,
                    uri_str: spotify_uri.to_string(),
                    spotify_uri: spotify_uri.clone().into(), 
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
        Ok((track, cover_id, l_track.album))
    }
}

impl SpotifyMetadata for LibrespotMetadataFetcher {

    fn fetch_album(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_album = futures::executor::block_on(
            librespot_metadata::album::Album::get(&self.session, spotify_uri))?;

        tracing::info!("Fetched album metadata: {}", l_album.name);

        let tracks = l_album.tracks()
            .map(|track_uri| {
                let (track, _, _) = self.fetch_single_track(&track_uri)?;
                Ok(track)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let cover_id = l_album.covers.first().map(|c| c.id.to_string()).unwrap_or_default();

        Ok(TrackCollection { 
            uri_str: l_album.id.to_string(), 
            spotify_id: l_album.id.to_id()?, 
            spotify_uri: Some(l_album.id.clone()),
            title: l_album.name, 
            artists: l_album.artists.iter().map(|a| a.name.clone()).collect(),
            total_tracks: tracks.len(),
            cover_id: Some(cover_id.clone()), 
            tracks: tracks, 
            upc: l_album.external_ids.iter().find(|id| id.external_type == "upc").map(|id| id.id.clone()),
            popularity: Some(l_album.popularity),
            label: Some(l_album.label),
            date: Some(l_album.date.to_string()),
        })
    }

    fn fetch_track(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let (track, cover_id, l_album) = self.fetch_single_track(spotify_uri)?;
        Ok(TrackCollection { 
            uri_str: l_album.id.to_string(), 
            spotify_id: l_album.id.to_id()?, 
            spotify_uri: Some(l_album.id.clone()),
            title: l_album.name, 
            artists: track.artists.clone(),
            total_tracks: 1, 
            cover_id: Some(cover_id.clone()), 
            tracks: vec![track], 
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

        let tracks = l_playlist.tracks()
            .map(|track_uri| {
                let (track, _, _) = self.fetch_single_track(&track_uri)?;
                Ok(track)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let artists = tracks.iter().flat_map(|t| t.artists.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        Ok(TrackCollection { 
            uri_str: l_playlist.id.to_string(), 
            spotify_id: l_playlist.id.to_id()?, 
            spotify_uri: Some(l_playlist.id.clone()),
            title: l_playlist.attributes.name.clone(), 
            artists: artists,
            total_tracks: 1, 
            cover_id: None, 
            tracks: tracks, 
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

        let tracks = l_show.episodes
            .iter()
            .map(|episode_uri| {
                let (track, _) = self.fetch_single_episode(&episode_uri)?;
                println!("Fetched episode metadata: {}", track.title);
                Ok(track)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let artists = tracks.iter().flat_map(|t| t.artists.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let cover_id = l_show.covers.first().map(|c| c.id.to_string()).unwrap_or_default();

        Ok(TrackCollection { 
            uri_str: l_show.id.to_string(), 
            spotify_id: l_show.id.to_id()?, 
            spotify_uri: Some(l_show.id.clone()),
            title: l_show.name.clone(), 
            artists: artists,
            total_tracks: tracks.len(), 
            cover_id: Some(cover_id), 
            tracks: tracks, 
            upc: None,
            popularity: None,
            label: Some(l_show.publisher.clone()),
            date: None,
        })
    }

    fn fetch_episode(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let (track, cover_id) = self.fetch_single_episode(spotify_uri)?;
        let track_clone = track.clone();
        Ok(TrackCollection { 
            uri_str: track.uri_str.clone(), 
            spotify_id: track.spotify_id, 
            spotify_uri: track.spotify_uri.clone(),
            title: track.title.clone(), 
            artists: track.artists.clone(),
            total_tracks: 1, 
            cover_id: Some(cover_id.clone()), 
            tracks: vec![track_clone], 
            upc: None,
            popularity: None,
            label: None,
            date: None,
        })
    }

}
