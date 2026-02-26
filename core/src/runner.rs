//! Production [`JobRunner`] implementation.
//!
//! [`DownloadRunner`] is the real entry point for processing a queued track:
//!
//! 1. Fetches track metadata.
//! 2. Checks whether the file already exists on disk (skip-if-present for free
//!    resume capability via a glob over `{stem}.*`).
//! 3. Fetches cover art bytes (non-fatal — reused for embedded tags and
//!    collection cover).
//! 4. Downloads and decrypts the audio file into a temporary file.
//! 5. Persists the audio to the final structured path under `output_dir`.
//! 6. Writes metadata tags via lofty — non-fatal.
//! 7. Records the track in the per-collection [`CollectionState`].
//! 8. When the **last** track in a collection completes, writes:
//!    - An M3U8 playlist (all collection types except single-track/episode).
//!    - A `cover.jpg` alongside the playlist (single cover for Album/Show,
//!      composite for Playlist).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::{
    container::CollectionType,
    coordinator::{JobRunner, ProgressUpdate, TrackInfo, WorkerApis},
    output_path::{build_output_dir, build_output_path, build_output_stem, safe_component},
    playlist::{self, TrackEntry},
    queue::QueueEntry,
    registry::TaskStatus,
    tagger,
};

// ---------------------------------------------------------------------------
// Per-collection completion tracking
// ---------------------------------------------------------------------------

struct CollectionState {
    /// Total number of tracks enqueued for this collection.
    total: usize,
    /// Tracks processed (success **or** failure).
    done: usize,
    /// Tracks that downloaded successfully, keyed by track URI so we can emit
    /// them in the original `track_uris` order when building the playlist.
    entries: HashMap<String, TrackEntry>,
    /// Cover-art bytes collected for the composite (up to 5 unique covers).
    cover_bytes: Vec<Vec<u8>>,
    /// Set of cover IDs already present in `cover_bytes` (deduplication).
    cover_ids_seen: HashSet<String>,
}

impl CollectionState {
    fn new(total: usize) -> Self {
        Self {
            total,
            done: 0,
            entries: HashMap::new(),
            cover_bytes: Vec::new(),
            cover_ids_seen: HashSet::new(),
        }
    }

    fn is_complete(&self) -> bool {
        self.done >= self.total
    }
}

// ---------------------------------------------------------------------------
// DownloadRunner
// ---------------------------------------------------------------------------

/// Production job runner.  Create one instance per [`TokioQueue`] and pass it
/// to [`TokioQueue::new`].
///
/// [`TokioQueue`]: crate::queue_tokio::TokioQueue
pub struct DownloadRunner {
    pub output_dir: PathBuf,
    /// Shared per-collection state used to detect when the last track in a
    /// collection has finished and trigger playlist/cover finalisation.
    tracker: Arc<Mutex<HashMap<String, CollectionState>>>,
}

impl DownloadRunner {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            tracker: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl JobRunner for DownloadRunner {
    fn run(
        &self,
        entry: &QueueEntry,
        apis: &WorkerApis,
        progress_tx: &broadcast::Sender<ProgressUpdate>,
    ) -> anyhow::Result<Option<String>> {
        // Convenience: fire a progress update with an optional track_info payload.
        let emit = |status: TaskStatus, msg: &str, track_info: Option<TrackInfo>| {
            let _ = progress_tx.send(ProgressUpdate {
                task_id: entry.task_id,
                status,
                message: Some(msg.to_owned()),
                track_info,
            });
        };
        let handle = tokio::runtime::Handle::current();

        // ── Initialise collection state on the first task for this collection ──
        {
            let mut tracker = self.tracker.lock().unwrap();
            tracker
                .entry(entry.collection.uri_str.clone())
                .or_insert_with(|| CollectionState::new(entry.collection.track_uris.len()));
        }

        // ── 1. Fetch track metadata ──────────────────────────────────────────
        let track = apis.track_metadata.fetch_by_uri(&entry.track_uri)?;
        let cover_id = track.cover_id.clone().unwrap_or_default();
        info!(
            task_id = %entry.task_id,
            title = %track.title,
            duration_s = track.duration_ms / 1000,
            "Starting download",
        );

        // Emit resolved metadata so the frontend can replace placeholder titles.
        emit(TaskStatus::Running, "Running…", Some(TrackInfo {
            title: track.title.clone(),
            artists: track.artists.clone(),
            number: track.number,
            disc_number: track.disc_number,
            duration_ms: track.duration_ms,
        }));

        // ── 2. Filesystem probe: skip if already downloaded ──────────────────
        let stem = build_output_stem(&self.output_dir, &track, &entry.collection);
        let glob_pattern = format!("{}.*", stem.display());

        let existing: Option<PathBuf> = glob::glob(&glob_pattern)
            .map(|paths| paths.filter_map(|r| r.ok()).next())
            .unwrap_or(None);

        if let Some(existing_path) = existing {
            info!(path = %existing_path.display(), "Skipping already-downloaded track");
            let te = TrackEntry {
                final_path: existing_path,
                title: track.title.clone(),
                duration_ms: track.duration_ms,
            };
            self.record_track(entry, &cover_id, te, None)?;
            return Ok(Some("File already exists".into()));
        }

        // ── 3. Fetch cover art ───────────────────────────────────────────────
        emit(TaskStatus::Running, "Fetching cover art…", None);
        let cover_bytes: Option<Vec<u8>> =
            match handle.block_on(apis.cover.fetch_cover(&cover_id)) {
                Ok(bytes) => Some(bytes),
                Err(e) => {
                    warn!("Failed to fetch cover for {}: {}", track.title, e);
                    None
                }
            };

        // ── 4. Create output directory & download ────────────────────────────
        emit(TaskStatus::Running, "Downloading audio…", None);
        let output_dir = build_output_dir(&self.output_dir, &track, &entry.collection);
        std::fs::create_dir_all(&output_dir)?;

        let downloaded = apis.audio.download(&entry.track_uri, &output_dir, &|failed_attempt, max, wait_ms| {
            let secs = (wait_ms + 999) / 1000;
            emit(TaskStatus::Retrying, &format!("Retry ({failed_attempt}/{max}) in {secs}s"), None);
        })?;
        debug!(
            task_id = %entry.task_id,
            bytes = downloaded.file.as_file().metadata().map(|m| m.len()).unwrap_or(0),
            format = ?downloaded.format,
            "Audio downloaded to temp file",
        );

        // ── 5. Persist to final path ─────────────────────────────────────────
        let final_path = build_output_path(
            &self.output_dir,
            &track,
            &entry.collection,
            downloaded.format,
        );
        downloaded.file.persist(&final_path).map_err(|e| {
            anyhow::anyhow!("Failed to persist audio to {}: {}", final_path.display(), e)
        })?;
        info!(path = %final_path.display(), "Saved audio");

        // ── 6. Embed metadata tags ───────────────────────────────────────────
        emit(TaskStatus::Running, "Writing tags…", None);
        if let Err(e) = tagger::write_tags(
            &final_path,
            &track,
            &entry.collection,
            cover_bytes.as_deref(),
            downloaded.replay_gain,
        ) {
            warn!("Failed to write tags to {}: {}", final_path.display(), e);
        } else {
            debug!(path = %final_path.display(), "Tags written");
        }

        // ── 7. Record and maybe finalise ─────────────────────────────────────
        let te = TrackEntry {
            final_path,
            title: track.title.clone(),
            duration_ms: track.duration_ms,
        };
        self.record_track(entry, &cover_id, te, cover_bytes.as_deref())?;

        Ok(Some("Downloaded".into()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl DownloadRunner {
    /// Store a completed track in the collection state and, if this was the
    /// last track for the collection, trigger playlist/cover finalisation.
    fn record_track(
        &self,
        entry: &QueueEntry,
        cover_id: &str,
        track_entry: TrackEntry,
        cover_bytes: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        let should_finalise;
        let total_cover_bytes: Vec<Vec<u8>>;
        let track_uris_order: Vec<String>;

        {
            let mut tracker = self.tracker.lock().unwrap();
            let state = tracker
                .get_mut(&entry.collection.uri_str)
                .expect("CollectionState must be initialised before record_track");

            state.entries.insert(entry.track_uri.clone(), track_entry);
            state.done += 1;

            // Collect unique covers (up to 5, deduplicated by cover_id)
            if let Some(bytes) = cover_bytes {
                if state.cover_bytes.len() < 5
                    && state.cover_ids_seen.insert(cover_id.to_owned())
                {
                    state.cover_bytes.push(bytes.to_vec());
                }
            }

            should_finalise = state.is_complete();
            if should_finalise {
                total_cover_bytes = std::mem::take(&mut state.cover_bytes);
                track_uris_order = entry.collection.track_uris.clone();
            } else {
                total_cover_bytes = Vec::new();
                track_uris_order = Vec::new();
            }
        }

        if should_finalise {
            // Pull the entries HashMap out while not holding the lock
            let entries_map: HashMap<String, TrackEntry> = {
                let mut tracker = self.tracker.lock().unwrap();
                let state = tracker
                    .get_mut(&entry.collection.uri_str)
                    .expect("state present");
                std::mem::take(&mut state.entries)
            };

            self.finalise_collection(entry, entries_map, track_uris_order, total_cover_bytes);
        }

        Ok(())
    }

    /// Write the M3U8 playlist and `cover.jpg` for a completed collection.
    fn finalise_collection(
        &self,
        entry: &QueueEntry,
        entries_map: HashMap<String, TrackEntry>,
        track_uris_order: Vec<String>,
        cover_bytes: Vec<Vec<u8>>,
    ) {
        let collection = &entry.collection;

        match collection.collection_type {
            CollectionType::SingleTrack | CollectionType::SingleEpisode => {
                // Write just cover.jpg alongside the single file; no playlist.
                if let Some(te) = entries_map.values().next() {
                    let dir = te
                        .final_path
                        .parent()
                        .unwrap_or(Path::new("."));
                    write_cover_jpg(dir, &cover_bytes, false);
                }
                return;
            }
            _ => {}
        }

        // Build ordered track list (preserves original collection order)
        let ordered: Vec<TrackEntry> = track_uris_order
            .iter()
            .filter_map(|uri| entries_map.get(uri))
            .map(|te| TrackEntry {
                final_path: te.final_path.clone(),
                title: te.title.clone(),
                duration_ms: te.duration_ms,
            })
            .collect();

        if ordered.is_empty() {
            warn!(
                "All tracks failed for '{}'; skipping playlist",
                collection.title
            );
            return;
        }

        // Determine where the playlist and cover live
        let safe_title = safe_component(&collection.title);
        let (m3u8_dir, is_playlist) = match collection.collection_type {
            CollectionType::Playlist => {
                // Playlist gets its own subdirectory under output_dir
                let dir = self.output_dir.join(&safe_title);
                (dir, true)
            }
            _ => {
                // Album / Show: alongside the tracks in the album directory
                let dir = ordered[0]
                    .final_path
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.output_dir.clone());
                (dir, false)
            }
        };

        let m3u8_path = m3u8_dir.join(format!("{}.m3u8", safe_title));

        match playlist::write_m3u8(&m3u8_path, collection, &ordered) {
            Ok(()) => info!(path = %m3u8_path.display(), "Wrote playlist"),
            Err(e) => warn!("Failed to write playlist {}: {}", m3u8_path.display(), e),
        }

        write_cover_jpg(&m3u8_dir, &cover_bytes, is_playlist);
    }
}

/// Write `cover.jpg` into `dir`.
///
/// When `composite` is `true` and multiple distinct covers are present, a
/// composite image is generated via [`playlist::composite_cover`].
fn write_cover_jpg(dir: &Path, cover_bytes: &[Vec<u8>], composite: bool) {
    if cover_bytes.is_empty() {
        return;
    }

    let cover_path = dir.join("cover.jpg");

    let result: anyhow::Result<()> = (|| {
        std::fs::create_dir_all(dir)?;
        if composite && cover_bytes.len() > 1 {
            let jpeg = playlist::composite_cover(cover_bytes)?;
            std::fs::write(&cover_path, jpeg)?;
        } else {
            std::fs::write(&cover_path, &cover_bytes[0])?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => info!(path = %cover_path.display(), "Saved cover"),
        Err(e) => warn!("Failed to write cover to {}: {}", cover_path.display(), e),
    }
}
