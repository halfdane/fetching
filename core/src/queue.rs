//! Queue types.
//!
//! Lean module — only the data types that belong to the queue itself.
//! Worker/coordination types live in [`coordinator`](crate::coordinator),
//! registry types in [`registry`](crate::registry), and Spotify API
//! adapters in [`spotify_api`](crate::spotify_api).

use std::sync::Arc;

use uuid::Uuid;

use crate::container::TrackCollection;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

pub type TaskId = Uuid;

// ---------------------------------------------------------------------------
// Queue entry
// ---------------------------------------------------------------------------

/// One entry per **track URI**. The parent collection is shared via `Arc`
/// so no data is duplicated across entries that belong to the same album/playlist.
#[derive(Clone, Debug)]
pub struct QueueEntry {
    pub task_id: TaskId,
    pub track_uri: String,
    pub collection: Arc<TrackCollection>,
}

impl QueueEntry {
    pub fn new(track_uri: impl Into<String>, collection: Arc<TrackCollection>) -> Self {
        Self {
            task_id: Uuid::new_v4(),
            track_uri: track_uri.into(),
            collection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{CollectionType, TrackCollection};

    fn fake_collection() -> Arc<TrackCollection> {
        Arc::new(TrackCollection {
            uri_str: "spotify:album:test".to_string(),
            collection_type: CollectionType::Album,
            title: "Test".to_string(),
            artists: vec![],
            cover_id: None,
            upc: None,
            total_tracks: 1,
            label: None,
            date: None,
            track_uris: vec!["spotify:track:aaa".to_string()],
        })
    }

    #[test]
    fn entry_new_assigns_unique_task_ids() {
        let col = fake_collection();
        let a = QueueEntry::new("spotify:track:aaa", Arc::clone(&col));
        let b = QueueEntry::new("spotify:track:aaa", Arc::clone(&col));
        assert_ne!(a.task_id, b.task_id);
    }

    #[test]
    fn entry_new_stores_track_uri() {
        let col = fake_collection();
        let entry = QueueEntry::new("spotify:track:bbb", col);
        assert_eq!(entry.track_uri, "spotify:track:bbb");
    }
}
