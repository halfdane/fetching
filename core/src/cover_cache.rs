use std::path::{PathBuf};

use crate::{container::TrackCollection, spotify_api::SpotifyCover};
use anyhow::Result;

pub struct CoverCache<C: SpotifyCover + Send + Sync + 'static> {
    base_dir: PathBuf,
    cover_fetcher: C
}

impl<C: SpotifyCover + Send + Sync + 'static> CoverCache<C> {
    pub async fn new(base_dir: PathBuf, cover_fetcher: C) -> anyhow::Result<Self> {
        Ok(Self { base_dir, cover_fetcher })
    }

    /// Warm cache for a single container: dedup unique cover IDs from tracks + container,
    /// fetch missing in parallel. Handles playlist multi-album case.
    pub async fn warm_from_container(&self, container: &TrackCollection) -> Result<()> {
        let unique_covers = self.collect_unique_covers(container);
        let mut handles: Vec<tokio::task::JoinHandle<std::result::Result<(), anyhow::Error>>> = vec![];

        for cover_id in unique_covers {
            let cover_fetcher = self.cover_fetcher.clone(); // If cover_fetcher is Arc or Clone
            let cover_path = self.get_cover_path_from_id(&cover_id);

            if !cover_path.exists() {
                handles.push(tokio::spawn(async move {
                    match cover_fetcher.fetch_cover(&cover_id).await {
                        Ok(bytes) => {
                            let dir_path = cover_path.parent().unwrap();
                            std::fs::create_dir_all(&dir_path)?;


                            tokio::fs::write(&cover_path, bytes).await?;
                            tracing::info!("Fetched and cached cover: {}", cover_id);
                            Ok(())
                        },
                        Err(e) => {
                            tracing::warn!("Failed to fetch album cover art: {}", e);
                            Err(e)
                        }
                    }
                }));
            }
        }

        // Await all parallel fetches
        for handle in handles {
            handle.await??;
        }
        Ok(())
    }

    fn collect_unique_covers(&self, collection: &TrackCollection) -> Vec<String> {
        let mut covers = std::collections::HashSet::new();
        
        // Container-level cover
        if let Some(id) = &collection.cover_id {
            covers.insert(id.clone());
        }
        
        // Per-track covers (playlists/albums with variants)
        for track in &collection.tracks {
            if let Some(id) = &track.cover_id {
                covers.insert(id.clone());
            }
        }
        
        covers.into_iter().collect()
    }

    fn get_cover_path_from_id(&self, cover_id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.jpg", cover_id))
    }

}

// Updated unit tests
#[cfg(test)]
mod tests {
    use crate::container::Track;

    use super::*;
    use async_trait::async_trait;
    use tempfile::TempDir;
    use tokio::fs;

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
            cover_id: Some(format!("track_cover_id{}", track_id)),
            language: vec!["en".to_string()],
            isrc: Some("trackISRC".to_string()),
            spotify_uri: None,
            date: "2020-01-01".to_string(),
            popularity: Some(50),
            disc_number: Some(1),
            number: 7,
        }.rehydrate().unwrap()
    }

    fn fake_collection(tracks: Vec<Track>) -> TrackCollection {
        TrackCollection { 
            spotify_id: ALBUM_ID.to_string(),
            uri_str: format!("spotify:album:{}", ALBUM_ID),
            title: "Test Album".to_string(), 
            artists: vec!["Album Artist".to_string()],
            cover_id: Some(format!("album_cover_id{}", ALBUM_ID)),
            total_tracks: 1, 
            tracks: tracks, 
            upc: Some("albumUPC".to_string()),
            popularity: Some(80),
            label: Some("Test Label".to_string()),
            date: Some("2020-01-01".to_string()),
            spotify_uri: None,
        }.rehydrate().unwrap()
    }

    #[derive(Clone)]

    struct MockCoverFetcher;
    #[async_trait]
    impl SpotifyCover for MockCoverFetcher {
        async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>> {
            // Simulate fetch by returning dummy data
            Ok(format!("fake_cover_data_for_{}", cover_id).into_bytes())
        }
    }

    #[tokio::test]
    async fn test_warm_multi_cover_playlist() {
        let client: MockCoverFetcher = MockCoverFetcher;
        let temp_dir = TempDir::new().unwrap();
        let cache = CoverCache::new(temp_dir.path().join("covers"), client).await.unwrap();

        let collection = fake_collection(
            vec![fake_track(TRACK_ID_1), fake_track(TRACK_ID_2)]);

        let unique = cache.collect_unique_covers(&collection);
        assert_eq!(unique.len(), 3);

        // Simulate: write one missing
        let path1 = cache.get_cover_path_from_id("abc123");
        let dir_path = path1.parent().unwrap();
        std::fs::create_dir_all(&dir_path).unwrap();

        fs::write(&path1, b"exists").await.unwrap();

        cache.warm_from_container(&collection).await.unwrap();

        // Verify paths created (mocked fetches would populate)
        assert!(cache.get_cover_path_from_id(&fake_track(TRACK_ID_1).cover_id.unwrap()).exists());
        assert!(cache.get_cover_path_from_id(&fake_track(TRACK_ID_2).cover_id.unwrap()).exists());
        assert!(cache.get_cover_path_from_id(&collection.cover_id.unwrap()).exists());
        // abc123 skipped
    }

    #[tokio::test]
    async fn test_dedup_single_album() {
        let client = MockCoverFetcher;
        let temp_dir = TempDir::new().unwrap();
        let cache = CoverCache::new(temp_dir.path().join("covers"), client).await.unwrap();
        let track1 = fake_track(TRACK_ID_1);
        let mut track2 = fake_track(TRACK_ID_2);

        track2.cover_id = track1.cover_id.clone(); // Force same cover ID for dedup test

        let collection = fake_collection(
            vec![track1, track2]);

        let unique = cache.collect_unique_covers(&collection);
        assert_eq!(unique.len(), 2); // Deduped
    }
}
