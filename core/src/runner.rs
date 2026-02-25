//! Production [`JobRunner`] implementation.
//!
//! [`DownloadRunner`] is the real entry point for processing a queued track:
//!
//! 1. Fetches track metadata.
//! 2. Fetches cover art bytes (non-fatal — used for both embedded tags and cover.jpg).
//! 3. Downloads and decrypts the audio file into a temporary file.
//! 4. Persists the audio to the final structured path under `output_dir`.
//! 5. Writes metadata tags (title, artists, album, track number, date, ISRC,
//!    label, embedded cover art) via lofty — non-fatal.
//! 6. Writes cover art as `cover.jpg` in the album directory — non-fatal.

use std::path::PathBuf;

use tracing::{debug, info, warn};

use crate::{
    output_path::{build_output_dir, build_output_path},
    queue::{JobRunner, QueueEntry, WorkerApis},
    tagger,
};

// ---------------------------------------------------------------------------
// DownloadRunner
// ---------------------------------------------------------------------------

/// Production job runner.  Create one instance per [`TokioQueue`] and pass it
/// to [`TokioQueue::new`].
///
/// [`TokioQueue`]: crate::queue_tokio::TokioQueue
pub struct DownloadRunner {
    pub output_dir: PathBuf,
}

impl DownloadRunner {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self { output_dir: output_dir.into() }
    }
}

impl JobRunner for DownloadRunner {
    fn run(&self, entry: &QueueEntry, apis: &WorkerApis) -> anyhow::Result<()> {
        let handle = tokio::runtime::Handle::current();

        // 1. Fetch track metadata
        let (track, cover_id) = apis.track_metadata.fetch_by_uri(&entry.track_uri)?;
        info!(
            task_id = %entry.task_id,
            title = %track.title,
            duration_s = track.duration_ms / 1000,
            "Starting download",
        );

        // 2. Fetch cover art bytes early so they can be embedded in tags and
        //    written as cover.jpg.  Non-fatal: None means tagging proceeds
        //    without art and no cover.jpg is written.
        let cover_bytes: Option<Vec<u8>> = match handle.block_on(apis.cover.fetch_cover(&cover_id)) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                warn!("Failed to fetch cover for {}: {}", track.title, e);
                None
            }
        };

        // 3. Create the output directory before downloading so the temp file
        //    lands on the same filesystem, enabling a cheap atomic rename(2).
        let output_dir = build_output_dir(&self.output_dir, &track, &entry.collection);
        std::fs::create_dir_all(&output_dir)?;

        // 4. Download audio → temp file placed in the output directory
        //    (decrypt + header strip inside).
        let downloaded = apis.audio.download(&entry.track_uri, &output_dir)?;
        debug!(
            task_id = %entry.task_id,
            bytes = downloaded.file.as_file().metadata().map(|m| m.len()).unwrap_or(0),
            format = ?downloaded.format,
            "Audio downloaded to temp file",
        );

        // 5. Persist to final structured path.
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

        // 6. Embed metadata tags.  Non-fatal: a tagging failure never removes
        //    an already-saved audio file.
        if let Err(e) = tagger::write_tags(
            &final_path,
            &track,
            &entry.collection,
            cover_bytes.as_deref(),
        ) {
            warn!("Failed to write tags to {}: {}", final_path.display(), e);
        } else {
            debug!(path = %final_path.display(), "Tags written");
        }

        // 7. Write cover.jpg alongside the audio file (reuse already-fetched bytes).
        let album_dir = final_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.output_dir.clone());
        let cover_path = album_dir.join("cover.jpg");

        if let Some(bytes) = &cover_bytes {
            match std::fs::write(&cover_path, bytes) {
                Ok(()) => info!(path = %cover_path.display(), bytes = bytes.len(), "Saved cover"),
                Err(e) => warn!("Failed to write cover to {}: {}", cover_path.display(), e),
            }
        }

        Ok(())
    }
}
