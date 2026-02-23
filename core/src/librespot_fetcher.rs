use librespot_core::session::Session;
use librespot_core::{SpotifyUri};
use librespot_metadata::{Metadata};

use crate::container::{TrackCollection, Track};
use crate::metadata::SpotifyMetadata;

pub struct LibrespotFetcher {
    session: Session,
}

impl LibrespotFetcher {
    pub async fn new(session: &Session) -> anyhow::Result<Self> {
        Ok(Self { session: session.clone() })
    }
    
    fn fetch_single_track(&self, spotify_uri: &SpotifyUri) -> Result<(Track, String, librespot_metadata::Album), anyhow::Error> {
        let l_track = futures::executor::block_on(
            librespot_metadata::track::Track::get(&self.session, spotify_uri))?;
        
        tracing::info!("Fetched track metadata: {}", l_track.name);
        let cover_id = l_track.album.covers.first().map(|c| c.id.to_string()).unwrap_or_default();
        let mut track = Track { 
                    spotify_id: spotify_uri.to_id()?,
                    uri_str: spotify_uri.to_string(),
                    spotify_uri: None, 
                    title: l_track.name, 
                    artists: l_track.artists.iter().map(|a| a.name.clone()).collect(), 
                    duration_ms: l_track.duration, 
                    cover_id: Some(cover_id.clone()),
                    explicit: l_track.is_explicit, 
                    language: l_track.language_of_performance, 
                    isrc: l_track.external_ids.iter().find(|id| id.external_type == "isrc").map(|id| id.id.clone()),

                    date: l_track.earliest_live_timestamp.to_string(),
                    popularity: l_track.popularity,
                    disc_number: l_track.disc_number,
                    number: l_track.number,
                };
        Ok((track.rehydrate()?, cover_id, l_track.album))
    }
}

impl SpotifyMetadata for LibrespotFetcher {
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
            total_tracks: 1, 
            cover_id: Some(cover_id.clone()), 
            tracks: tracks, 
            upc: l_album.external_ids.iter().find(|id| id.external_type == "upc").map(|id| id.id.clone()),
            popularity: l_album.popularity,
            label: l_album.label,
            date: l_album.date.to_string(),
        }.rehydrate()?)
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
            popularity: l_album.popularity,
            label: l_album.label,
            date: l_album.date.to_string(),
        }.rehydrate()?)
    }

    // fn fetch_playlist(&self, uri: &SpotifyUri) -> Result<PlaylistMetadata> {
    //     let playlist_id = SpotifyId::from_uri(uri)?.try_into()?;
    //     let playlist = futures::executor::block_on(metadata::Playlist::load(&self.session, playlist_id))?;
        
    //     let mut tracks = vec![];
    //     for page in playlist.list()?.pages() {
    //         let page = futures::executor::block_on(page)?;
    //         for item in page.items {
    //             if let Ok(track_id) = item.try_into() {
    //                 let track = futures::executor::block_on(metadata::Track::load(&self.session, track_id))?;
    //                 tracks.push(TrackMetadata {
    //                     title: track.name().to_string(),
    //                     artists: track.artists().iter().map(|a| a.name().to_string()).collect(),
    //                     duration_ms: track.duration as u32 * 1000,
    //                     explicit: None,
    //                     cover_id: track.album().and_then(|a| a.cover_id()).map(|id| id.to_base62()).unwrap_or_default(),
    //                 });
    //             }
    //         }
    //     }
        
    //     Ok(PlaylistMetadata {
    //         name: playlist.name().to_string(),
    //         tracks,
    //     })
    // }

    // fn fetch_episode(&self, uri: &SpotifyUri) -> Result<EpisodeMetadata> {
    //     // Librespot podcast/episode support is limited; fallback to track-like
    //     // Real impl: use session.content_feeder() or extend metadata
    //     self.fetch_track(uri).map(|track_meta| EpisodeMetadata {
    //         name: track_meta.title,
    //         show_artists: track_meta.artists,
    //         duration_ms: track_meta.duration_ms,
    //         chapters: None, // Requires additional API
    //         explicit: false,
    //         language: None,
    //     })
    // }

    // fn fetch_show(&self, _uri: &SpotifyUri) -> Result<ShowMetadata> {
    //     // Shows limited; mock or extend
    //     Ok(ShowMetadata {
    //         name: "Podcast Show".to_string(),
    //         narrators: vec!["Host".to_string()],
    //         episodes: vec![],
    //         language: Some("en".to_string()),
    //     })
    // }
}
