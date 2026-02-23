use serde::{Deserialize, Serialize};
use librespot_core::SpotifyUri;
use anyhow::{Result};


#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    // Identifiers
    pub uri_str: String,
    pub spotify_id: String,
    // Transient runtime (ignored in ser/de)
    #[serde(skip_serializing, skip_deserializing)]
    pub spotify_uri: Option<SpotifyUri>,

    // Metadata
    pub title: String,
    pub artists: Vec<String>,
    pub cover_id: Option<String>,
    pub isrc: Option<String>,

    pub duration_ms: i32,
    pub disc_number: i32,
    pub number: i32,
    pub date: String,
    pub popularity: i32,
    
    // Spotify extras
    pub explicit: bool,
    pub language: Vec<String>,

}

impl Track {
    pub fn rehydrate(&mut self) -> Result<Self> {
        self.spotify_uri = Some(SpotifyUri::from_uri(&self.uri_str)?);
        Ok(self.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TrackCollection {
    // Identifiers
    pub uri_str: String,
    pub spotify_id: String,
    // Transient runtime (ignored in ser/de)
    #[serde(skip_serializing, skip_deserializing)]
    pub spotify_uri: Option<SpotifyUri>,
    
    // Metadata
    pub title: String,
    pub artists: Vec<String>,
    pub cover_id: Option<String>,
    pub upc: Option<String>,
    pub total_tracks: usize,
    pub popularity: i32,
    pub label: String,
    pub date: String,

    pub tracks: Vec<Track>,
}

impl TrackCollection {
    pub fn rehydrate(&mut self) -> Result<Self> {
        self.spotify_uri = Some(SpotifyUri::from_uri(&self.uri_str)?);
        for track in &mut self.tracks {
            track.rehydrate()?;
        }
        Ok(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::metadata::SpotifyMetadata;
    use crate::metadata::fetch_collection;

    use super::*;
    use librespot_core::FileId;
    use pretty_assertions::{assert_eq};

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
            spotify_uri: None,
            date: "2020-01-01".to_string(),
            popularity: 50,
            disc_number: 1,
            number: 7,
        }.rehydrate().unwrap()
    }

    fn fake_collection(tracks: Vec<Track>) -> TrackCollection {
        TrackCollection { 
            spotify_id: ALBUM_ID.to_string(),
            uri_str: format!("spotify:album:{}", ALBUM_ID),
            title: "Test Album".to_string(), 
            artists: vec!["Album Artist".to_string()],
            cover_id: Some("album_cover_id".to_string()),
            total_tracks: 1, 
            tracks: tracks, 
            upc: Some("albumUPC".to_string()),
            popularity: 80,
            label: "Test Label".to_string(),
            date: "2020-01-01".to_string(),
            spotify_uri: None,
        }.rehydrate().unwrap()
    }
    
    
    struct MockFetcher;
    impl SpotifyMetadata for MockFetcher {
        fn fetch_album(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> { 
            Ok(fake_collection(vec![fake_track(TRACK_ID_1)])) 
        }

        fn fetch_track(&self, _uri: &SpotifyUri) -> anyhow::Result<TrackCollection> { 
            Ok(fake_collection(vec![fake_track(TRACK_ID_1), fake_track(TRACK_ID_2)])) 
        }

        // fn fetch_playlist(&self, _uri: &SpotifyUri) -> anyhow::Result<PlaylistMetadata> { Ok(PlaylistMetadata { /* mock */ }) }
        // fn fetch_episode(&self, _uri: &SpotifyUri) -> anyhow::Result<EpisodeMetadata> { Ok(EpisodeMetadata { name: "Test Ep".to_string(), show_artists: vec![], duration_ms: 1800000, chapters: Some(vec![]), explicit: true, language: Some("en".to_string()) }) }
        // fn fetch_show(&self, _uri: &SpotifyUri) -> anyhow::Result<ShowMetadata> { Ok(ShowMetadata { name: "Test Show".to_string(), narrators: vec![], episodes: vec![], language: Some("en".to_string()) }) }
    }


        #[test]
    fn test_dispatch_single_track() {
        let fetcher = MockFetcher;
        let container = fetch_collection(&format!("spotify:track:{}", TRACK_ID_1), &fetcher).unwrap(); 

        println!("Container: {:?}", container);

        assert_eq!(container.total_tracks, 1);
        assert_eq!(container.uri_str, format!("spotify:album:{}", ALBUM_ID));
        assert_eq!(container.spotify_id, ALBUM_ID);
        assert_eq!(container.title, "Test Album");

        assert_eq!(container.tracks[0].duration_ms, 180000);
        assert_eq!(container.tracks[0].uri_str, format!("spotify:track:{}", TRACK_ID_1));
        assert_eq!(container.tracks[0].spotify_id, TRACK_ID_1);
        assert_eq!(container.tracks[0].title, "Test Track");
        assert_eq!(container.tracks[0].artists, vec!["Track Artist".to_string()]);
        assert_eq!(container.tracks[0].explicit, false);
    }

    #[test]
    fn test_serde() {
        let fetcher = MockFetcher;
        let container = fetch_collection(&format!("spotify:track:{}", TRACK_ID_1), &fetcher).unwrap();
        let serialized = serde_json::to_vec(&container).unwrap();
        let mut deserialized: TrackCollection = serde_json::from_slice(&serialized).unwrap();
        deserialized.rehydrate().unwrap();
        assert_eq!(deserialized, container);
    }

    #[test]
    fn test_fileid() {
        let bytes: [u8; 20] = [0x01, 0x02, 0x03, 0x04, 0x05,
                       0x06, 0x07, 0x08, 0x09, 0x0A,
                       0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
                       0x10, 0x11, 0x12, 0x13, 0x14];
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
