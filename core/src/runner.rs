//! Production [`JobRunner`] implementation.
//!
//! [`DownloadRunner`] is the real entry point for processing a queued track:
//!
//! 1. Fetches track metadata.
//! 2. Downloads and decrypts the audio file into a temporary file.
//! 3. Persists the audio to the final structured path under `output_dir`.
//! 4. Fetches the cover image and writes it as `cover.jpg` in the album directory
//!    (silently skips on error so a single missing thumbnail never aborts a track).

use std::path::PathBuf;

use tracing::{debug, info, warn};

use crate::{
    output_path::build_output_path,
    queue::{JobRunner, QueueEntry, WorkerApis},
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

        // 2. Download audio → temp file (decrypt + header strip inside)
        let downloaded = apis.audio.download(&entry.track_uri)?;
        debug!(
            task_id = %entry.task_id,
            bytes = downloaded.file.as_file().metadata().map(|m| m.len()).unwrap_or(0),
            format = ?downloaded.format,
            "Audio downloaded to temp file",
        );

        // 3. Persist to final structured path  (TODO: lofty tagging before this step)
        let final_path = build_output_path(
            &self.output_dir,
            &track,
            &entry.collection,
            downloaded.format,
        );
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        downloaded.file.persist(&final_path).map_err(|e| {
            anyhow::anyhow!("Failed to persist audio to {}: {}", final_path.display(), e)
        })?;
        info!(path = %final_path.display(), "Saved audio");

        // 4. Fetch cover and write as cover.jpg in the album directory
        //    Non-fatal: a missing cover never aborts the track.
        let album_dir = final_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.output_dir.clone());
        let cover_path = album_dir.join("cover.jpg");

        match handle.block_on(apis.cover.fetch_cover(&cover_id)) {
            Ok(bytes) => match std::fs::write(&cover_path, &bytes) {
                Ok(()) => info!(path = %cover_path.display(), bytes = bytes.len(), "Saved cover"),
                Err(e) => warn!("Failed to write cover to {}: {}", cover_path.display(), e),
            },
            Err(e) => warn!("Failed to fetch cover for {}: {}", track.title, e),
        }

        Ok(())
    }
}
