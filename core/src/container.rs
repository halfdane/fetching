use serde::{Deserialize, Serialize};
use librespot_core::SpotifyUri;
use anyhow::{Result, bail};

use crate::metadata::SpotifyMetadata;

// Core enums/structs
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum MediaType {
    Music,
    Episode,
    Audiobook,
    AudioPlay,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Chapter {
    pub start_ms: u32,
    pub title: String,
}

// SINGLE Container - unified type owns everything!
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Container {
    // Identifiers (serializable)
    pub uri_str: String,
    pub spotify_id: String,
    
    // Metadata (serializable)
    pub title: String,
    pub artists_or_creators: Vec<String>,
    pub total_tracks: usize,
    pub cover_path: String,
    pub media_type: MediaType,
    
    // Tracks (serializable) 
    pub tracks: Vec<Track>,
    
    // Transient runtime (ignored in ser/de)
    #[serde(skip_serializing, skip_deserializing)]
    pub spotify_uri: Option<SpotifyUri>,
}

impl Container {
    pub fn rehydrate(&mut self) -> Result<()> {
        self.spotify_uri = Some(SpotifyUri::from_uri(&self.uri_str)?);
        for track in &mut self.tracks {
            track.rehydrate()?;
        }
        Ok(())
    }
    
    pub fn container_progress(&self) -> f32 {
        if self.total_tracks == 0 { 0.0 }
        else {
            self.tracks.iter().map(|t| t.progress).sum::<f32>() / self.total_tracks as f32
        }
    }
    
    pub fn is_fully_loaded(&self) -> bool {
        self.tracks.iter().all(|t| t.progress >= 1.0)
    }
    
    pub fn completed_tracks(&self) -> usize {
        self.tracks.iter().filter(|t| t.progress >= 1.0).count()
    }
}

// Track - everything serializable
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub uri_str: String,
    pub spotify_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub duration_ms: i32,
    pub media_type: MediaType,
    pub progress: f32,
    
    // Audio
    pub cover_id: Option<String>,
    pub audio_path: Option<String>,  // Tagged file path
    
    // Spotify extras
    pub chapters: Option<Vec<Chapter>>,
    pub explicit: bool,
    pub language: Vec<String>,

    // Transient runtime (ignored in ser/de)
    #[serde(skip_serializing, skip_deserializing)]
    pub spotify_uri: Option<SpotifyUri>,
}

impl Track {
    pub fn rehydrate(&mut self) -> Result<()> {
        self.spotify_uri = Some(SpotifyUri::from_uri(&self.uri_str)?);
        Ok(())
    }
}

// // Raw metadata structs (trait return types)
// #[derive(Clone)]
// pub struct AlbumMetadata {
//     pub name: String,
//     pub artists: Vec<String>,
//     pub total_tracks: usize,
//     pub cover_id: String,
//     pub tracks: Vec<TrackMetadata>,
// }

// #[derive(Clone)]
// pub struct TrackMetadata {
//     pub title: String,
//     pub artists: Vec<String>,
//     pub duration_ms: u32,
//     pub explicit: Option<bool>,
//     pub cover_id: String,
// }

// #[derive(Clone)]
// pub struct PlaylistMetadata {
//     pub name: String,
//     pub creator: String,
//     pub cover_id: String,
//     pub tracks: Vec<TrackMetadata>,
// }

// #[derive(Clone)]
// pub struct EpisodeMetadata {
//     pub name: String,
//     pub show_name: String,
//     pub show_artists: Vec<String>,
//     pub duration_ms: u32,
//     pub cover_id: String,
//     pub chapters: Option<Vec<Chapter>>,
//     pub explicit: bool,
//     pub language: Option<String>,
// }

// #[derive(Clone)]
// pub struct ShowMetadata {
//     pub name: String,
//     pub publisher: String,
//     pub cover_id: String,
//     pub episodes: Vec<EpisodeMetadata>,
// }

// Container factory - dispatch + constructors
pub fn dispatch_container(uri_str: &str, fetcher: &impl SpotifyMetadata) -> Result<Container> {
    let spotify_uri = SpotifyUri::from_uri(uri_str)?;
    
    let container = match spotify_uri.item_type() {
        // "album" => Container::new_album(uri_str, fetcher)?,
        "track" => Container::new_single_track(&spotify_uri, fetcher)?,
        // "playlist" => Container::new_playlist(uri_str, fetcher)?,
        // "episode" => Container::new_episode(uri_str, fetcher)?,
        // "show" => Container::new_show(uri_str, fetcher)?,
        _ => bail!("Unsupported URI type: {}", uri_str),
    };
    
    Ok(container)
}

// Constructors
impl Container {
    // fn new_album(uri_str: &str, fetcher: &impl SpotifyMetadata) -> Result<Self> {
    //     let spotify_uri = SpotifyUri::from_uri(uri_str)?;
    //     let meta = fetcher.fetch_album(&spotify_uri)?;
        
    //     let tracks = meta.tracks.into_iter().enumerate().map(|(idx, t)| Track {
    //         id: format!("track_{}", idx), // Real: extract SpotifyId
    //         uri: format!("spotify:track:{}", t.cover_id), // Real ID
    //         title: t.title,
    //         artists: t.artists,
    //         duration_ms: t.duration_ms,
    //         media_type: MediaType::Music,
    //         progress: 0.0,
    //         cover_id: Some(meta.cover_id.clone()),
    //         chapters: None,
    //         explicit: t.explicit.unwrap_or(false),
    //         language: None,
    //         audio_path: None,
    //     }).collect();
        
    //     Ok(Self {
    //         uri_str: uri_str.to_string(),
    //         spotify_id: spotify_uri.id()?.to_base62(),
    //         title: meta.name,
    //         artists_or_creators: meta.artists,
    //         total_tracks: meta.total_tracks,
    //         cover_path: format!("covers/{}", meta.cover_id),
    //         media_type: MediaType::Music,
    //         tracks,
    //         spotify_uri,
    //     })
    // }
    
    fn new_single_track(spotify_uri: &SpotifyUri, fetcher: &impl SpotifyMetadata) -> Result<Self> {
        let meta = fetcher.fetch_track(&spotify_uri)?;
        
        let track = Track {
            spotify_id: spotify_uri.to_id()?,
            uri_str: spotify_uri.to_string(),
            title: meta.title.clone(),
            artists: meta.artists.clone(),
            duration_ms: meta.duration_ms,
            media_type: MediaType::Music,
            progress: 0.0,
            cover_id: meta.cover_id.clone(),
            chapters: meta.chapters.clone(),
            explicit: meta.explicit.clone(),
            language: meta.language.clone(),
            audio_path: meta.audio_path.clone(),
            spotify_uri: Some(spotify_uri.clone()),
        };
        
        Ok(Self {
            uri_str: spotify_uri.to_string(),
            spotify_id: track.spotify_id.clone(),
            title: meta.title.clone(),
            artists_or_creators: meta.artists.clone(),
            total_tracks: 1,
            cover_path: meta.cover_id.map(|id| format!("covers/{}", id)).unwrap_or_else(|| "covers/default.jpg".to_string()),
            media_type: MediaType::Music,
            tracks: vec![track],
            spotify_uri: Some(spotify_uri.clone()),
        })
    }
    
    // fn new_playlist(uri_str: &str, fetcher: &impl SpotifyMetadata) -> Result<Self> {
    //     let spotify_uri = SpotifyUri::from_uri(uri_str)?;
    //     let meta = fetcher.fetch_playlist(&spotify_uri)?;
        
    //     let tracks: Vec<Track> = meta.tracks.into_iter().enumerate().map(|(idx, t)| Track {
    //         id: format!("playlist_track_{}", idx),
    //         uri: format!("spotify:track:{}", t.cover_id), 
    //         title: t.title,
    //         artists: t.artists,
    //         duration_ms: t.duration_ms,
    //         media_type: MediaType::Music,
    //         progress: 0.0,
    //         cover_id: Some(t.cover_id),  // Per-track covers!
    //         chapters: None,
    //         explicit: t.explicit.unwrap_or(false),
    //         language: None,
    //         audio_path: None,
    //     }).collect();
        
    //     Ok(Self {
    //         uri_str: uri_str.to_string(),
    //         spotify_id: spotify_uri.id()?.to_base62(),
    //         title: meta.name,
    //         artists_or_creators: vec![meta.creator],
    //         total_tracks: meta.tracks.len(),
    //         cover_path: format!("covers/{}", meta.cover_id),
    //         media_type: MediaType::Music,
    //         tracks,
    //         spotify_uri,
    //     })
    // }
    
    // fn new_episode(uri_str: &str, fetcher: &impl SpotifyMetadata) -> Result<Self> {
    //     let spotify_uri = SpotifyUri::from_uri(uri_str)?;
    //     let meta = fetcher.fetch_episode(&spotify_uri)?;
        
    //     let track = Track {
    //         id: spotify_uri.id()?.to_base62(),
    //         uri: uri_str.to_string(),
    //         title: meta.name,
    //         artists: meta.show_artists,
    //         duration_ms: meta.duration_ms,
    //         media_type: MediaType::Episode,
    //         progress: 0.0,
    //         cover_id: Some(meta.cover_id),
    //         chapters: meta.chapters,
    //         explicit: meta.explicit,
    //         language: meta.language,
    //         audio_path: None,
    //     };
        
    //     Ok(Self {
    //         uri_str: uri_str.to_string(),
    //         spotify_id: track.id.clone(),
    //         title: meta.name,
    //         artists_or_creators: meta.show_artists,
    //         total_tracks: 1,
    //         cover_path: format!("covers/{}", meta.cover_id),
    //         media_type: MediaType::Episode,
    //         tracks: vec![track],
    //         spotify_uri,
    //     })
    // }
    
    // fn new_show(uri_str: &str, fetcher: &impl SpotifyMetadata) -> Result<Self> {
    //     let spotify_uri = SpotifyUri::from_uri(uri_str)?;
    //     let meta = fetcher.fetch_show(&spotify_uri)?;
        
    //     // Flatten episodes to tracks
    //     let tracks: Vec<Track> = meta.episodes.into_iter().enumerate().map(|(idx, ep)| Track {
    //         id: format!("show_episode_{}", idx),
    //         uri: format!("spotify:episode:{}", ep.cover_id),
    //         title: ep.name,
    //         artists: vec![meta.publisher.clone()],
    //         duration_ms: ep.duration_ms,
    //         media_type: MediaType::Audiobook,
    //         progress: 0.0,
    //         cover_id: Some(meta.cover_id.clone()),
    //         chapters: ep.chapters,
    //         explicit: ep.explicit,
    //         language: ep.language,
    //         audio_path: None,
    //     }).collect();
        
    //     Ok(Self {
    //         uri_str: uri_str.to_string(),
    //         spotify_id: spotify_uri.id()?.to_base62(),
    //         title: meta.name,
    //         artists_or_creators: vec![meta.publisher],
    //         total_tracks: tracks.len(),
    //         cover_path: format!("covers/{}", meta.cover_id),
    //         media_type: MediaType::Audiobook,
    //         tracks,
    //         spotify_uri,
    //     })
    // }
}



#[cfg(test)]
mod tests {
    use super::*;
    use librespot_core::FileId;
    use pretty_assertions::{assert_eq};

    const VALID_ID_1: &str = "6rqhFgbbKwnb9MLmUQDhG6";
    const VALID_ID_2: &str = "12l8e8JfVOgX7jQewjyNbU";
    
    struct MockFetcher;
    impl SpotifyMetadata for MockFetcher {
        // fn fetch_album(&self, _uri: &SpotifyUri) -> anyhow::Result<AlbumMetadata> {
        //     Ok(AlbumMetadata { /* mock data */ name: "Test Album".to_string(), artists: vec!["Artist".to_string()], total_tracks: 10, tracks: vec![] })
        // }
            fn fetch_track(&self, uri: &SpotifyUri) -> anyhow::Result<Track> { 
            Ok(Track {
                spotify_id: "theSpotifyId".to_string(),
                uri_str: "spotify:track:theSpotifyId".to_string(),
                title: "Test Track".to_string(), 
                artists: vec!["Artist".to_string()], 
                duration_ms: 180000, 
                explicit: false,
                media_type: MediaType::Music,
                progress: 0.0,
                cover_id: Some("test_cover_id".to_string()),
                audio_path: Some("test audio path".to_string()),
                chapters: None,
                language: Some("en".to_string()),
                spotify_uri: Some(uri.clone()),
                }) 
            }
        // fn fetch_playlist(&self, _uri: &SpotifyUri) -> anyhow::Result<PlaylistMetadata> { Ok(PlaylistMetadata { /* mock */ }) }
        // fn fetch_episode(&self, _uri: &SpotifyUri) -> anyhow::Result<EpisodeMetadata> { Ok(EpisodeMetadata { name: "Test Ep".to_string(), show_artists: vec![], duration_ms: 1800000, chapters: Some(vec![]), explicit: true, language: Some("en".to_string()) }) }
        // fn fetch_show(&self, _uri: &SpotifyUri) -> anyhow::Result<ShowMetadata> { Ok(ShowMetadata { name: "Test Show".to_string(), narrators: vec![], episodes: vec![], language: Some("en".to_string()) }) }
    }


        #[test]
    fn test_dispatch_single_track() {
        let fetcher = MockFetcher;
        let container = dispatch_container(&format!("spotify:track:{}", VALID_ID_1), &fetcher).unwrap(); 

        println!("Container: {:?}", container);

        assert_eq!(container.total_tracks, 1);
        assert_eq!(container.tracks[0].media_type, MediaType::Music);
        assert_eq!(container.uri_str, format!("spotify:track:{}", VALID_ID_1));
        assert_eq!(container.spotify_id, VALID_ID_1);
        assert_eq!(container.title, "Test Track");
    }

    #[test]
    fn test_serde() {
        let fetcher = MockFetcher;
        let container = dispatch_container(&format!("spotify:track:{}", VALID_ID_1), &fetcher).unwrap();
        let serialized = serde_json::to_vec(&container).unwrap();
        let mut deserialized: Container = serde_json::from_slice(&serialized).unwrap();
        deserialized.rehydrate().unwrap(); // Reconstruct SpotifyUri from uri_str
        assert_eq!(deserialized, container); // No SpotifyUri, serializes fine
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
