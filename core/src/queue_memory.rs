//! In-memory `QueueStorage` implementation.
//!
//! Backed by a `Mutex<VecDeque>` — no persistence, entries are lost on restart.
//! Useful for tests and the `batch` subcommand where durability isn't needed.
//!
//! # Usage
//!
//! ```ignore
//! let queue = TokioQueue::new(apis, runner); // uses InMemoryStorage by default
//! // or explicitly:
//! let queue = TokioQueue::with_storage(InMemoryStorage::new(), apis, runner);
//! ```

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::queue::{QueueEntry, QueueStorage};

/// FIFO in-memory queue storage.
///
/// Replace with [`SledStorage`](crate::queue_sled::SledStorage) for durable
/// storage that survives server restarts.
pub struct InMemoryStorage {
    entries: Mutex<VecDeque<QueueEntry>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueStorage for InMemoryStorage {
    fn push(&self, entry: QueueEntry) -> anyhow::Result<()> {
        self.entries
            .lock()
            .expect("InMemoryStorage mutex poisoned")
            .push_back(entry);
        Ok(())
    }

    fn pop(&self) -> anyhow::Result<Option<QueueEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("InMemoryStorage mutex poisoned")
            .pop_front())
    }

    fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("InMemoryStorage mutex poisoned")
            .len()
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
    fn storage_pop_on_empty_returns_none() {
        let s = InMemoryStorage::new();
        assert!(s.pop().unwrap().is_none());
    }

    #[test]
    fn storage_preserves_fifo_order() {
        let s = InMemoryStorage::new();
        let col = fake_collection(&["uri:a", "uri:b", "uri:c"]);
        for uri in ["uri:a", "uri:b", "uri:c"] {
            s.push(QueueEntry::new(uri, Arc::clone(&col))).unwrap();
        }
        assert_eq!(s.pop().unwrap().unwrap().track_uri, "uri:a");
        assert_eq!(s.pop().unwrap().unwrap().track_uri, "uri:b");
        assert_eq!(s.pop().unwrap().unwrap().track_uri, "uri:c");
        assert!(s.pop().unwrap().is_none());
    }
}
