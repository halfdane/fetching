use librespot_core::session::Session;
use librespot_core::{SpotifyId, SpotifyUri, spotify_id};
use librespot_metadata::{self as metadata, Metadata};

use crate::container::{MediaType, Track};
use crate::metadata::SpotifyMetadata;

pub struct LibrespotFetcher {
    session: Session,
}

impl LibrespotFetcher {
    pub async fn new(session: &Session) -> anyhow::Result<Self> {
        Ok(Self { session: session.clone() })
    }
}

impl SpotifyMetadata for LibrespotFetcher {
    // fn fetch_album(&self, uri: &SpotifyUri) -> Result<AlbumMetadata> {
    //     let album_id = SpotifyId::from_uri(uri)?.try_into()?;
        
    //     // Use metadata crate to fetch
    //     let album = futures::executor::block_on(metadata::Album::load(&self.session, album_id))?;
        
    //     let mut tracks = vec![];
    //     for page in album.list()?.pages() {
    //         let page = futures::executor::block_on(page)?;
    //         for item in page.items {
    //             let track = futures::executor::block_on(metadata::Track::load(&self.session, item.try_into()?))?;
    //             tracks.push(TrackMetadata {
    //                 title: track.name().to_string(),
    //                 artists: track.artists().iter().map(|a| a.name().to_string()).collect(),
    //                 duration_ms: track.duration as u32 * 1000,
    //                 explicit: None, // Fetch from content attrs if needed
    //                 cover_id: album.cover_id().map(|id| id.to_base62()).unwrap_or_default(),
    //             });
    //         }
    //     }
        
    //     Ok(AlbumMetadata {
    //         name: album.name().to_string(),
    //         artists: album.artists().iter().map(|a| a.name().to_string()).collect(),
    //         total_tracks: album.num_tracks() as usize,
    //         tracks,
    //         cover_url: format!("https://i.scdn.co/image/ab67616d00001e02/{}", album.cover_id().map(|id| id.to_base62()).unwrap_or_default()),
    //     })
    // }

    fn fetch_track(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<Track> {
        let l_track = futures::executor::block_on(
            librespot_metadata::track::Track::get(&self.session, spotify_uri))?;
        
        let cover_id = l_track.album.covers.first().map(|cover| cover.id); // Get cover ID if available

        let album_id = l_track.album.id;
        Ok(Track { 
            spotify_id: spotify_uri.to_id()?,
            uri_str: spotify_uri.to_string(),
            title: l_track.name, 
            artists: l_track.artists.iter().map(|a| a.name.clone()).collect(), 
            duration_ms: l_track.duration, 
            media_type: MediaType::Music, 
            progress: 0.0, 
            cover_id: cover_id.map(|id| id.to_string()), 
            audio_path: None, 
            chapters: None, 
            explicit: l_track.is_explicit, 
            language: l_track.language_of_performance, 
            spotify_uri: None, 
        })
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
