use librespot_core::SpotifyUri;
use serde::{Deserialize, Serialize};


#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]

pub enum CollectionType {
    Album, Playlist, Show, SingleTrack, SingleEpisode,
}

pub fn to_collection_type(spotify_uri: &SpotifyUri) -> anyhow::Result<CollectionType> {
    match spotify_uri.item_type().to_lowercase().as_str() {
        "album" => Ok(CollectionType::Album),
        "playlist" => Ok(CollectionType::Playlist),
        "show" => Ok(CollectionType::Show),
        "track" => Ok(CollectionType::SingleTrack),
        "episode" => Ok(CollectionType::SingleEpisode),
        _ => anyhow::bail!("Unsupported URI type for Track-Collection: {}", spotify_uri),

    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    // Identifiers
    pub uri_str: String,

    // Metadata
    pub title: String,
    pub artists: Vec<String>,
    pub cover_id: Option<String>,
    pub isrc: Option<String>,

    pub duration_ms: i32,
    pub disc_number: Option<i32>,
    pub number: Option<i32>,
    pub date: Option<String>,

    // Spotify extras
    pub explicit: bool,
    pub language: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TrackCollection {
    // Identifiers
    pub uri_str: String,

    pub collection_type: CollectionType,

    // Metadata
    pub title: String,
    pub artists: Vec<String>,
    pub cover_id: Option<String>,
    pub upc: Option<String>,
    pub total_tracks: usize,
    pub label: Option<String>,
    pub date: Option<String>,

    pub track_uris: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify_api::SpotifyCollectionMetadata;
    use librespot_core::SpotifyUri;
    use pretty_assertions::assert_eq;

    const TRACK_ID_1: &str = "6rqhFgbbKwnb9MLmUQDhG6";
    const TRACK_ID_2: &str = "63vL5oxWrlvaJ0ayNaQnbX";
    const ALBUM_ID: &str = "12l8e8JfVOgX7jQewjyNbU";

    fn fake_track(track_id: &str) -> Track {
        Track {
            uri_str: format!("spotify:track:{}", track_id),
            title: "Test Track".to_string(),
            artists: vec!["Track Artist".to_string()],
            duration_ms: 180000,
            explicit: false,
            cover_id: Some("track_cover_id".to_string()),
            language: vec!["en".to_string()],
            isrc: Some("trackISRC".to_string()),
            date: Some("2020-01-01".to_string()),
            disc_number: Some(1),
            number: Some(7),
        }
    }

    fn fake_collection(track_uris: Vec<String>) -> TrackCollection {
        TrackCollection {
            uri_str: format!("spotify:album:{}", ALBUM_ID),
            title: "Test Album".to_string(),
            artists: vec!["Album Artist".to_string()],
            cover_id: Some("album_cover_id".to_string()),
            total_tracks: track_uris.len(),
            track_uris,
            upc: Some("albumUPC".to_string()),
            label: Some("Test Label".to_string()),
            date: Some("2020-01-01".to_string()),
            collection_type: CollectionType::Album,
        }
    }

    struct MockFetcher;
    impl SpotifyCollectionMetadata for MockFetcher {
        fn fetch_album(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(TrackCollection {
                collection_type: CollectionType::Album,
                ..fake_collection(vec![
                    format!("spotify:track:{}", TRACK_ID_1),
                    format!("spotify:track:{}", TRACK_ID_2),
                ])
            })
        }
        fn fetch_track(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(TrackCollection {
                collection_type: CollectionType::SingleTrack,
                ..fake_collection(vec![format!("spotify:track:{}", TRACK_ID_1)])
            })
        }
        fn fetch_playlist(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(TrackCollection {
                collection_type: CollectionType::Playlist,
                ..fake_collection(vec![
                    format!("spotify:track:{}", TRACK_ID_1),
                    format!("spotify:track:{}", TRACK_ID_2),
                ])
            })
        }
        fn fetch_show(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(TrackCollection {
                collection_type: CollectionType::Show,
                ..fake_collection(vec![
                    format!("spotify:track:{}", TRACK_ID_1),
                    format!("spotify:track:{}", TRACK_ID_2),
                ])
            })
        }
        fn fetch_episode(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(TrackCollection {
                collection_type: CollectionType::SingleEpisode,
                ..fake_collection(vec![format!("spotify:track:{}", TRACK_ID_1)])
            })
        }
    }

    // --- fetch_by_uri dispatch ---

    #[test]
    fn should_dispatch_album_uri_to_fetch_album() {
        // given
        let fetcher = MockFetcher;
        // when
        let result = fetcher.fetch_by_uri(&format!("spotify:album:{}", ALBUM_ID)).unwrap();
        // then
        assert_eq!(result.collection_type, CollectionType::Album);
        assert_eq!(result.total_tracks, 2);
    }

    #[test]
    fn should_dispatch_track_uri_to_fetch_single_track() {
        // given
        let fetcher = MockFetcher;
        // when
        let result = fetcher.fetch_by_uri(&format!("spotify:track:{}", TRACK_ID_1)).unwrap();
        // then
        assert_eq!(result.collection_type, CollectionType::SingleTrack);
        assert_eq!(result.total_tracks, 1);
    }

    #[test]
    fn should_dispatch_playlist_uri_to_fetch_playlist() {
        // given
        let fetcher = MockFetcher;
        // when
        let result = fetcher.fetch_by_uri(&format!("spotify:playlist:{}", ALBUM_ID)).unwrap();
        // then
        assert_eq!(result.collection_type, CollectionType::Playlist);
        assert_eq!(result.total_tracks, 2);
    }

    #[test]
    fn should_dispatch_show_uri_to_fetch_show() {
        // given
        let fetcher = MockFetcher;
        // when
        let result = fetcher.fetch_by_uri(&format!("spotify:show:{}", ALBUM_ID)).unwrap();
        // then
        assert_eq!(result.collection_type, CollectionType::Show);
        assert_eq!(result.total_tracks, 2);
    }

    #[test]
    fn should_dispatch_episode_uri_to_fetch_episode() {
        // given
        let fetcher = MockFetcher;
        // when
        let result = fetcher.fetch_by_uri(&format!("spotify:episode:{}", TRACK_ID_1)).unwrap();
        // then
        assert_eq!(result.collection_type, CollectionType::SingleEpisode);
        assert_eq!(result.total_tracks, 1);
    }

    #[test]
    fn should_reject_unsupported_uri_type() {
        // given
        let fetcher = MockFetcher;
        // when
        let result = fetcher.fetch_by_uri(&format!("spotify:artist:{}", ALBUM_ID));
        // then
        assert!(result.is_err());
    }

    // --- to_collection_type ---

    #[test]
    fn should_classify_all_collection_uri_types() {
        // given / when / then
        let cases = [
            (format!("spotify:album:{}", ALBUM_ID),    CollectionType::Album),
            (format!("spotify:playlist:{}", ALBUM_ID), CollectionType::Playlist),
            (format!("spotify:show:{}", ALBUM_ID),     CollectionType::Show),
            (format!("spotify:track:{}", TRACK_ID_1),  CollectionType::SingleTrack),
            (format!("spotify:episode:{}", TRACK_ID_1), CollectionType::SingleEpisode),
        ];
        for (uri_str, expected) in cases {
            let uri = SpotifyUri::from_uri(&uri_str).unwrap();
            assert_eq!(to_collection_type(&uri).unwrap(), expected, "failed for {uri_str}");
        }
    }

    // --- serde ---

    #[test]
    fn should_round_trip_track_serde() {
        // given
        let track = fake_track(TRACK_ID_1);
        // when
        let serialized = serde_json::to_vec(&track).unwrap();
        let deserialized: Track = serde_json::from_slice(&serialized).unwrap();
        // then
        assert_eq!(deserialized, track);
    }

    #[test]
    fn should_round_trip_track_serde_with_no_optional_fields() {
        // given
        let track = Track {
            uri_str: format!("spotify:track:{}", TRACK_ID_1),
            title: "Minimal".to_string(),
            artists: vec![],
            duration_ms: 0,
            explicit: false,
            cover_id: None,
            language: vec![],
            isrc: None,
            date: None,
            disc_number: None,
            number: Some(1),
        };
        // when
        let serialized = serde_json::to_vec(&track).unwrap();
        let deserialized: Track = serde_json::from_slice(&serialized).unwrap();
        // then
        assert_eq!(deserialized, track);
    }

    #[test]
    fn should_round_trip_collection_serde() {
        // given
        let collection = fake_collection(vec![
            format!("spotify:track:{}", TRACK_ID_1),
            format!("spotify:track:{}", TRACK_ID_2),
        ]);
        // when
        let serialized = serde_json::to_vec(&collection).unwrap();
        let deserialized: TrackCollection = serde_json::from_slice(&serialized).unwrap();
        // then
        assert_eq!(deserialized, collection);
    }

    #[test]
    fn should_round_trip_collection_serde_with_no_optional_fields() {
        // given
        let collection = TrackCollection {
            uri_str: format!("spotify:album:{}", ALBUM_ID),
            collection_type: CollectionType::Album,
            title: "Minimal".to_string(),
            artists: vec![],
            cover_id: None,
            upc: None,
            total_tracks: 0,
            label: None,
            date: None,
            track_uris: vec![],
        };
        // when
        let serialized = serde_json::to_vec(&collection).unwrap();
        let deserialized: TrackCollection = serde_json::from_slice(&serialized).unwrap();
        // then
        assert_eq!(deserialized, collection);
    }
}
