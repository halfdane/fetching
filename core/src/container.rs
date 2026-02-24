use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    // Identifiers
    pub uri_str: String,
    pub spotify_id: String,

    // Metadata
    pub title: String,
    pub artists: Vec<String>,
    pub cover_id: Option<String>,
    pub isrc: Option<String>,

    pub duration_ms: i32,
    pub disc_number: Option<i32>,
    pub number: i32,
    pub date: String,
    pub popularity: Option<i32>,

    // Spotify extras
    pub explicit: bool,
    pub language: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TrackCollection {
    // Identifiers
    pub uri_str: String,
    pub spotify_id: String,

    // Metadata
    pub title: String,
    pub artists: Vec<String>,
    pub cover_id: Option<String>,
    pub upc: Option<String>,
    pub total_tracks: usize,
    pub popularity: Option<i32>,
    pub label: Option<String>,
    pub date: Option<String>,

    pub track_uris: Vec<String>,
}

#[cfg(test)]
mod tests {
    use crate::spotify_api::SpotifyCollectionMetadata;

    use super::*;
    use librespot_core::FileId;
    use librespot_core::SpotifyUri;
    use pretty_assertions::assert_eq;

    const TRACK_ID_1: &str = "6rqhFgbbKwnb9MLmUQDhG6";
    const TRACK_ID_2: &str = "63vL5oxWrlvaJ0ayNaQnbX";
    const ALBUM_ID: &str = "12l8e8JfVOgX7jQewjyNbU";

    fn fake_track(track_id: &str) -> Track {
        Track {
            spotify_id: track_id.to_string(),
            uri_str: format!("spotify:track:{}", track_id),
            title: "Test Track".to_string(),
            artists: vec!["Track Artist".to_string()],
            duration_ms: 180000,
            explicit: false,
            cover_id: Some("track_cover_id".to_string()),
            language: vec!["en".to_string()],
            isrc: Some("trackISRC".to_string()),
            date: "2020-01-01".to_string(),
            popularity: Some(50),
            disc_number: Some(1),
            number: 7,
        }
    }

    fn fake_collection(track_uris: Vec<String>) -> TrackCollection {
        TrackCollection {
            spotify_id: ALBUM_ID.to_string(),
            uri_str: format!("spotify:album:{}", ALBUM_ID),
            title: "Test Album".to_string(),
            artists: vec!["Album Artist".to_string()],
            cover_id: Some("album_cover_id".to_string()),
            total_tracks: 1,
            track_uris,
            upc: Some("albumUPC".to_string()),
            popularity: Some(80),
            label: Some("Test Label".to_string()),
            date: Some("2020-01-01".to_string()),
        }
    }

    struct MockFetcher;
    impl SpotifyCollectionMetadata for MockFetcher {
        fn fetch_album(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(fake_collection(vec![format!(
                "spotify:track:{}",
                TRACK_ID_1
            )]))
        }

        fn fetch_track(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(fake_collection(vec![
                format!("spotify:track:{}", TRACK_ID_1),
                format!("spotify:track:{}", TRACK_ID_2),
            ]))
        }

        fn fetch_playlist(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(fake_collection(vec![
                format!("spotify:track:{}", TRACK_ID_1),
                format!("spotify:track:{}", TRACK_ID_2),
            ]))
        }

        fn fetch_show(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(fake_collection(vec![
                format!("spotify:track:{}", TRACK_ID_1),
                format!("spotify:track:{}", TRACK_ID_2),
            ]))
        }

        fn fetch_episode(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> {
            Ok(fake_collection(vec![format!(
                "spotify:track:{}",
                TRACK_ID_1
            )]))
        }
    }

    #[test]
    fn test_dispatch_single_track() {
        let fetcher = MockFetcher;
        let container = fetcher
            .fetch_by_uri(&format!("spotify:track:{}", TRACK_ID_1))
            .unwrap();

        println!("Container: {:?}", container);

        assert_eq!(container.total_tracks, 1);
        assert_eq!(container.uri_str, format!("spotify:album:{}", ALBUM_ID));
        assert_eq!(container.spotify_id, ALBUM_ID);
        assert_eq!(container.title, "Test Album");

        assert_eq!(
            container.track_uris[0],
            format!("spotify:track:{}", TRACK_ID_1)
        );
    }

    #[test]
    fn test_serde() {
        let fetcher = MockFetcher;
        let container = fetcher
            .fetch_by_uri(&format!("spotify:track:{}", TRACK_ID_1))
            .unwrap();
        let serialized = serde_json::to_vec(&container).unwrap();
        let deserialized: TrackCollection = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(deserialized, container);
    }

    #[test]
    fn test_fileid() {
        let bytes: [u8; 20] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10, 0x11, 0x12, 0x13, 0x14,
        ];
        let file_id = FileId(bytes);
        // let file_id = FileId([/* 20 bytes */]);
        let hex_str = file_id.to_string(); // e.g. "abcdef123456..."

        use hex;

        // Convert hex string back to FileId
        let bytes = hex::decode(&hex_str).expect("valid hex");
        let new_file_id = FileId::from_raw(&bytes);

        // new_file_id is now equivalent to the original file_id
        println!("Original FileId: {:?}", file_id);
        println!("Hex String: {}", hex_str);
        println!("Reconstructed FileId: {:?}", new_file_id);

        assert_eq!(file_id, new_file_id);
    }
}
