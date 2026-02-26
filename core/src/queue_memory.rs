//! In-memory FIFO track queue.
//!
//! Backed by a `Mutex<VecDeque>` — entries are lost on restart.
//! On the server path, the `TaskRegistry` (sled) is the durable source of
//! truth; entries recovered at startup are re-pushed into this queue by the
//! coordinator.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::queue::QueueEntry;

/// Thread-safe, in-memory FIFO queue of track download entries.
pub struct TrackQueue {
    entries: Mutex<VecDeque<QueueEntry>>,
}

impl TrackQueue {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
        }
    }

    /// Append an entry to the back of the queue.
    pub fn push(&self, entry: QueueEntry) {
        self.entries
            .lock()
            .expect("TrackQueue mutex poisoned")
            .push_back(entry);
    }

    /// Remove and return the entry at the front, or `None` if empty.
    pub fn pop(&self) -> Option<QueueEntry> {
        self.entries
            .lock()
            .expect("TrackQueue mutex poisoned")
            .pop_front()
    }

    /// Number of entries currently waiting.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("TrackQueue mutex poisoned")
            .len()
    }

    /// Returns `true` if the queue has no pending entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for TrackQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{CollectionType, TrackCollection};
    use crate::queue::QueueEntry;
    use std::sync::Arc;

    fn fake_collection(uris: &[&str]) -> Arc<TrackCollection> {
        Arc::new(TrackCollection {
            uri_str: "spotify:album:test".to_string(),
            collection_type: CollectionType::Album,
            title: "Test".to_string(),
            artists: vec![],
            cover_id: None,
            upc: None,
            total_tracks: uris.len(),
            label: None,
            date: None,
            track_uris: uris.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn pop_on_empty_returns_none() {
        let q = TrackQueue::new();
        assert!(q.pop().is_none());
    }

    #[test]
    fn preserves_fifo_order() {
        let q = TrackQueue::new();
        let col = fake_collection(&["uri:a", "uri:b", "uri:c"]);
        for uri in ["uri:a", "uri:b", "uri:c"] {
            q.push(QueueEntry::new(uri, Arc::clone(&col)));
        }
        assert_eq!(q.pop().unwrap().track_uri, "uri:a");
        assert_eq!(q.pop().unwrap().track_uri, "uri:b");
        assert_eq!(q.pop().unwrap().track_uri, "uri:c");
        assert!(q.pop().is_none());
    }
}
