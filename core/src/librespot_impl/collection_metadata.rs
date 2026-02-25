use std::sync::Arc;

use librespot_core::session::Session;
use librespot_core::{SpotifyUri};
use librespot_metadata::Metadata;

use crate::container::{CollectionType, TrackCollection};
use crate::spotify_api::{SpotifyCollectionMetadata, SpotifyTrackMetadata};


pub struct LibrespotCollectionMetadataFetcher<T: SpotifyTrackMetadata> {
    pub session: Arc<Session>,
    pub track_fetcher: T,
}

impl<T: SpotifyTrackMetadata> LibrespotCollectionMetadataFetcher<T> {
    pub fn new(session: Arc<Session>, track_fetcher: T) -> Self {
        Self { session, track_fetcher }
    }
}

impl<T: SpotifyTrackMetadata> SpotifyCollectionMetadata
    for LibrespotCollectionMetadataFetcher<T>
{
    fn fetch_album(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_album = futures::executor::block_on(librespot_metadata::album::Album::get(
            &self.session,
            spotify_uri,
        ))?;

        tracing::info!("Fetched album metadata: {}", l_album.name);

        let track_uris = l_album
            .tracks()
            .map(|uri| uri.to_string())
            .collect::<Vec<_>>();

        let cover_id = l_album
            .covers
            .first()
            .map(|c| c.id.to_string())
            .unwrap_or_default();

        Ok(TrackCollection {
            uri_str: l_album.id.to_string(),
            collection_type: CollectionType::Album,
            title: l_album.name,
            artists: l_album.artists.iter().map(|a| a.name.clone()).collect(),
            total_tracks: track_uris.len(),
            cover_id: Some(cover_id.clone()),
            track_uris: track_uris,
            upc: l_album
                .external_ids
                .iter()
                .find(|id| id.external_type == "upc")
                .map(|id| id.id.clone()),
            label: Some(l_album.label),
            date: Some(l_album.date.to_string()),
        })
    }

    fn fetch_track(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        // Fetch the track — album name, covers and date are embedded in l_track.album,
        // so no separate Album::get call is needed (and passing a track URI to Album::get
        // causes librespot to hang trying to resolve it as an album ID).
        let l_track = futures::executor::block_on(
            librespot_metadata::track::Track::get(&self.session, spotify_uri),
        )?;

        tracing::info!("Fetched single-track metadata: {}", l_track.name);

        let cover_id = l_track
            .album
            .covers
            .first()
            .map(|c| c.id.to_string())
            .unwrap_or_default();

        Ok(TrackCollection {
            uri_str: spotify_uri.to_string(),
            collection_type: CollectionType::SingleTrack,
            title: l_track.name,
            artists: l_track.artists.iter().map(|a| a.name.clone()).collect(),
            total_tracks: 1,
            cover_id: Some(cover_id),
            track_uris: vec![spotify_uri.to_string()],
            upc: None, // UPC is an album-level identifier; not available on track metadata
            label: None, // label requires a full album fetch; not needed for a single track
            date: Some(l_track.album.date.to_string()),
        })
    }

    fn fetch_playlist(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_playlist = futures::executor::block_on(librespot_metadata::playlist::Playlist::get(
            &self.session,
            spotify_uri,
        ))?;

        tracing::info!("Fetched playlist metadata: {}", l_playlist.attributes.name);

        let track_uris = l_playlist
            .tracks()
            .map(|uri| uri.to_string())
            .collect::<Vec<_>>();
        Ok(TrackCollection {
            uri_str: l_playlist.id.to_string(),
            collection_type: CollectionType::Playlist,
            title: l_playlist.attributes.name.clone(),
            artists: vec![],
            total_tracks: track_uris.len(),
            cover_id: None,
            track_uris: track_uris,
            upc: None,
            label: None,
            date: None,
        })
    }

    fn fetch_show(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let l_show = futures::executor::block_on(librespot_metadata::show::Show::get(
            &self.session,
            spotify_uri,
        ))?;

        tracing::info!("Fetched show metadata: {}", l_show.name);

        let track_uris = l_show
            .episodes
            .iter()
            .map(|uri| uri.to_string())
            .collect::<Vec<_>>();

        let cover_id = l_show
            .covers
            .first()
            .map(|c| c.id.to_string())
            .unwrap_or_default();

        Ok(TrackCollection {
            uri_str: l_show.id.to_string(),
            title: l_show.name.clone(),
            artists: vec![],
            total_tracks: track_uris.len(),
            cover_id: Some(cover_id),
            track_uris: track_uris,
            upc: None,
            label: Some(l_show.publisher.clone()),
            date: None,
            collection_type: CollectionType::Show,
        })
    }

    fn fetch_episode(&self, spotify_uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
        let track = self.track_fetcher.fetch_single_episode(spotify_uri)?;
        // Use the show name (stored in track.artists[0]) as the collection title so the
        // directory mirrors a full-show download. Falls back to the episode title if the
        // show name is somehow absent.
        let show_name = track.artists.first().cloned().unwrap_or_else(|| track.title.clone());
        Ok(TrackCollection {
            uri_str: track.uri_str.clone(),
            title: show_name,
            artists: track.artists.clone(),
            total_tracks: 1,
            cover_id: track.cover_id.clone(),
            track_uris: vec![track.uri_str.clone()],
            upc: None,
            label: None,
            date: track.date.clone(),
            collection_type: CollectionType::SingleEpisode,
        })
    }
}
