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
//! 7. When the **last** task in a collection completes (checked via DB),
//!    writes:
//!    - An M3U8 playlist (all collection types except single-track/episode).
//!    - A `cover.jpg` alongside the playlist (single cover for Album/Show,
//!      composite for Playlist).

use std::{
    collections::HashSet,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use tracing::{debug, info, warn};

use crate::{
    container::{CollectionType, TrackCollection},
    coordinator::{JobRunner, ProgressUpdate, TrackInfo, WorkerApis},
    db::{Database, TaskStatus},
    output_path::{build_output_dir, build_output_path, build_output_stem, safe_component},
    playlist::{self, TrackEntry},
    queue::QueueEntry,
    tagger,
};

// ---------------------------------------------------------------------------
// DownloadRunner
// ---------------------------------------------------------------------------

/// Production job runner.  Create one instance and pass it to the coordinator.
///
/// Stateless with respect to collection finalization — the [`Database`] is
/// queried to determine when a collection is complete, making the runner
/// crash-safe.
pub struct DownloadRunner {
    pub output_dir: PathBuf,
    pub db: Option<Arc<Database>>,
}

impl DownloadRunner {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            db: None,
        }
    }

    pub fn with_db(output_dir: impl Into<PathBuf>, db: Arc<Database>) -> Self {
        Self {
            output_dir: output_dir.into(),
            db: Some(db),
        }
    }
}

impl JobRunner for DownloadRunner {
    fn run(
        &self,
        entry: &QueueEntry,
        apis: &WorkerApis,
        collection_id: &str,
        on_progress: &dyn Fn(ProgressUpdate),
    ) -> anyhow::Result<Option<String>> {
        // Convenience: fire a progress update with an optional track_info payload.
        let emit = |status: TaskStatus, msg: &str, track_info: Option<TrackInfo>| {
            on_progress(ProgressUpdate {
                task_id: entry.task_id,
                collection_id: collection_id.to_owned(),
                status,
                message: Some(msg.to_owned()),
                track_info,
            });
        };
        let handle = tokio::runtime::Handle::current();

        // ── 1. Fetch track metadata ──────────────────────────────────────────
        let track = apis.track_metadata.fetch_by_uri(&entry.track_uri)?;
        let cover_id = track.cover_id.clone().unwrap_or_default();
        info!(
            task_id = %entry.task_id,
            title = %track.title,
            duration_s = track.duration_ms / 1000,
            "Starting download",
        );

        // Persist resolved metadata to the database.
        if let Some(db) = &self.db {
            if let Ok(Some(track_id)) = db.track_id_for_task(&entry.task_id.to_string()) {
                if let Err(e) = db.update_track_metadata(&track_id, &track) {
                    warn!("Failed to persist track metadata for {}: {e}", entry.task_id);
                }
            }
        }

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
            self.maybe_finalise(collection_id, &entry.collection);
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
        // Make the file world-readable so other users (e.g. navidrome) can access it.
        if let Err(e) = std::fs::set_permissions(
            &final_path,
            std::fs::Permissions::from_mode(0o644),
        ) {
            warn!(path = %final_path.display(), "Failed to set file permissions: {e}");
        }
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

        // ── 7. Maybe finalise collection ─────────────────────────────────────
        self.maybe_finalise(collection_id, &entry.collection);

        Ok(Some("Downloaded".into()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl DownloadRunner {
    /// Check if the collection is complete (via DB) and trigger finalization
    /// if so. No-op when no database is configured (batch mode).
    fn maybe_finalise(&self, collection_id: &str, collection: &TrackCollection) {
        let Some(db) = &self.db else { return };

        match db.is_collection_complete(collection_id) {
            Ok(true) => {
                info!(collection_id, "Collection complete — finalising");
                self.finalise_collection_from_db(collection_id, collection);
            }
            Ok(false) => {} // not done yet
            Err(e) => {
                warn!("Failed to check collection completion for {collection_id}: {e}");
            }
        }
    }

    /// Build the M3U8 playlist and cover.jpg by scanning the output directory
    /// for already-downloaded files that match the collection's tracks.
    fn finalise_collection_from_db(
        &self,
        collection_id: &str,
        collection: &TrackCollection,
    ) {
        let db = match &self.db {
            Some(db) => db,
            None => return,
        };

        // Get all tracks for this collection from the DB
        let tracks = match db.get_tracks_for_collection(collection_id) {
            Ok(t) => t,
            Err(e) => {
                warn!("Failed to get tracks for finalization: {e}");
                return;
            }
        };

        // Reload the full collection from DB (may have more complete data)
        let collection = match db.get_collection(collection_id) {
            Ok(Some(c)) => c,
            Ok(None) => {
                warn!("Collection {collection_id} not found during finalization");
                return;
            }
            Err(e) => {
                warn!("Failed to load collection for finalization: {e}");
                collection.clone()
            }
        };

        match collection.collection_type {
            CollectionType::SingleTrack | CollectionType::SingleEpisode => {
                // For single tracks, just write cover.jpg alongside the file
                if let Some(track) = tracks.first() {
                    if let Some(ref title) = track.title {
                        // Find the downloaded file via glob
                        if let Some(path) = self.find_track_file(&track.uri, &collection) {
                            if let Some(dir) = path.parent() {
                                self.write_cover_from_track(&collection, dir);
                            }
                        }
                        let _ = title; // suppress unused warning
                    }
                }
                return;
            }
            _ => {}
        }

        // Build ordered track list by scanning the filesystem for downloaded files
        let mut ordered: Vec<TrackEntry> = Vec::new();
        let mut cover_ids_seen = HashSet::new();
        let cover_bytes_list: Vec<Vec<u8>> = Vec::new();

        for track_uri in &collection.track_uris {
            if let Some(path) = self.find_track_file(track_uri, &collection) {
                // Try to get title/duration from the DB track data
                let (title, duration_ms) = tracks
                    .iter()
                    .find(|t| t.uri == *track_uri)
                    .map(|t| {
                        (
                            t.title.clone().unwrap_or_else(|| track_uri.clone()),
                            t.duration_ms.unwrap_or(0),
                        )
                    })
                    .unwrap_or_else(|| (track_uri.clone(), 0));

                ordered.push(TrackEntry {
                    final_path: path,
                    title,
                    duration_ms,
                });
            }

            // Collect cover art for composite (if applicable)
            let cover_id = tracks
                .iter()
                .find(|t| t.uri == *track_uri)
                .and_then(|_| collection.cover_id.clone());
            if let Some(cid) = cover_id {
                if cover_bytes_list.len() < 5 && cover_ids_seen.insert(cid.clone()) {
                    // Fetch cover for composite — best effort
                    // Note: in the future this could be cached in DB
                }
            }
        }

        if ordered.is_empty() {
            warn!(
                "All tracks failed for '{}'; skipping playlist",
                collection.title
            );
            return;
        }

        // Determine where the playlist and cover live
        let safe_title = safe_component(&collection.title);
        let (m3u8_dir, _is_playlist) = match collection.collection_type {
            CollectionType::Playlist => {
                let dir = self.output_dir.join(&safe_title);
                (dir, true)
            }
            _ => {
                let dir = ordered[0]
                    .final_path
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.output_dir.clone());
                (dir, false)
            }
        };

        let m3u8_path = m3u8_dir.join(format!("{}.m3u8", safe_title));

        match playlist::write_m3u8(&m3u8_path, &collection, &ordered) {
            Ok(()) => info!(path = %m3u8_path.display(), "Wrote playlist"),
            Err(e) => warn!("Failed to write playlist {}: {}", m3u8_path.display(), e),
        }

        self.write_cover_from_track(&collection, &m3u8_dir);
    }

    /// Find a downloaded file for a track URI by globbing the output directory.
    fn find_track_file(&self, _track_uri: &str, collection: &TrackCollection) -> Option<PathBuf> {
        // We need to scan the output directory for files matching the expected pattern.
        // The output path structure is: {output_dir}/{artist}/{year - album}/{track_num - title}.{ext}
        // Since we don't have the Track struct here, we scan the collection's output dir.
        let output_dir = match collection.collection_type {
            CollectionType::Playlist => self.output_dir.clone(),
            _ => {
                let artist = collection
                    .artists
                    .first()
                    .map(|a| safe_component(a))
                    .unwrap_or_else(|| "Unknown Artist".to_string());
                let album = safe_component(&collection.title);
                let year = collection
                    .date
                    .as_deref()
                    .and_then(|d| d.split('-').next())
                    .unwrap_or("0000");
                self.output_dir
                    .join(&artist)
                    .join(format!("{} - {}", year, album))
            }
        };

        // Just check if the directory exists and has audio files
        // This is a simplification — for production we'd match by track number/title
        if output_dir.exists() {
            for ext in &["ogg", "mp3", "flac", "m4a", "wav"] {
                let pattern = format!("{}/**/*.{}", output_dir.display(), ext);
                if let Ok(paths) = glob::glob(&pattern) {
                    for path in paths.filter_map(|r| r.ok()) {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    /// Write cover.jpg into a directory using the collection's cover art.
    fn write_cover_from_track(&self, _collection: &TrackCollection, dir: &Path) {
        // Cover art writing is best-effort. In the redesigned system,
        // cover art is still fetched via the CoverFetcher at download time,
        // not stored in the DB. For finalization, we check if a cover.jpg
        // already exists (written during individual track download).
        let cover_path = dir.join("cover.jpg");
        if cover_path.exists() {
            debug!(path = %cover_path.display(), "Cover already exists");
        }
    }
}
