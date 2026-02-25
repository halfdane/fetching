use std::sync::Arc;
use librespot_core::{Session, SpotifyUri};
use librespot_metadata::Metadata;

use crate::{container::Track, spotify_api::SpotifyTrackMetadata};


pub struct LibrespotTrackMetadataFetcher {
    pub session: Arc<Session>,
}

impl SpotifyTrackMetadata for LibrespotTrackMetadataFetcher {

    fn fetch_single_episode(
        &self,
        spotify_uri: &SpotifyUri,
    ) -> anyhow::Result<Track> {
        let l_episode = futures::executor::block_on(
            librespot_metadata::episode::Episode::get(&self.session, spotify_uri),
        )?;

        let cover_id = l_episode
            .covers
            .first()
            .map(|c| c.id.to_string())
            .unwrap_or_default();

        let track = Track {
            uri_str: spotify_uri.to_string(),
            title: l_episode.name,
            artists: vec![l_episode.show_name.clone()],
            duration_ms: l_episode.duration,
            cover_id: Some(cover_id.clone()),
            explicit: l_episode.is_explicit,
            language: vec![],
            isrc: None,

            date: {
                let s = l_episode.publish_time.to_string();
                if s.starts_with("0000") { None } else { Some(s) }
            },
            disc_number: None,
            number: if l_episode.number == 0 { None } else { Some(l_episode.number) },
        };
        Ok(track)
    }

    fn fetch_single_track(
        &self,
        spotify_uri: &SpotifyUri,
    ) -> anyhow::Result<Track> {
        let l_track = futures::executor::block_on(
            librespot_metadata::track::Track::get(&self.session, spotify_uri),
        )?;

        tracing::info!("Fetched track metadata: {}", l_track.name);

        let cover_id = l_track
            .album
            .covers
            .first()
            .map(|c| c.id.to_string())
            .unwrap_or_default();
        let track: Track = Track {
            uri_str: spotify_uri.to_string(),
            title: l_track.name,
            artists: l_track.artists.iter().map(|a| a.name.clone()).collect(),
            duration_ms: l_track.duration,
            cover_id: Some(cover_id.clone()),
            explicit: l_track.is_explicit,
            language: l_track.language_of_performance,
            isrc: l_track
                .external_ids
                .iter()
                .find(|id| id.external_type == "isrc")
                .map(|id| id.id.clone()),

            date: Some(l_track.album.date.to_string()),
            disc_number: Some(l_track.disc_number),
            number: Some(l_track.number),
        };
Ok(track)
    }
}



