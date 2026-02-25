//! Audio download trait, result type, and shared helpers.
//!
//! # Layers
//!
//! ```text
//! audio.rs            – AudioFileDownloader trait, DownloadedTrack, RetryConfig,
//!                       and the two pure functions used by every backend
//! audio_librespot.rs  – LibrespotAudioDownloader: FileId resolution + stream/decrypt/write
//! ```
//!
//! # Tagging flow (to be implemented with lofty)
//!
//! ```text
//!   JobRunner
//!     │  apis.audio.download(track_uri) → DownloadedTrack { file: NamedTempFile }
//!     │  lofty tags DownloadedTrack.file  (seekable, writable)
//!     │  std::fs::rename(file.path(), final_path)   ← atomic on same filesystem
//! ```
//!
//! Using a temp file rather than an in-memory buffer is deliberately chosen:
//! lofty must seek backwards to write Vorbis Comment / ID3 headers at the front
//! of the file after the audio body is written. A temp-then-rename strategy is
//! also crash-safe: the final path never contains a partially-written file.

use std::io::{Read, Write};

use librespot_metadata::audio::AudioFileFormat;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of a successful audio download, ready for lofty tagging.
///
/// The file contains decoded audio with Spotify's proprietary header already
/// stripped (OGG only). Drop this value to delete the temp file, or call
/// `file.persist(final_path)` to move it into place atomically.
pub struct DownloadedTrack {
    pub track_uri: String,
    /// The audio format, used to derive the correct file extension.
    pub format: AudioFileFormat,
    /// Seekable, writable temp file on the same filesystem as the final destination.
    pub file: NamedTempFile,
}

// ---------------------------------------------------------------------------
// Retry configuration
// ---------------------------------------------------------------------------

/// Controls retry behaviour for transient audio download errors.
///
/// Pass to `LibrespotAudioDownloader::with_retry` or accept the `Default`
/// Retry parameters for transient audio download failures.
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of attempts (includes the first try).
    pub max_attempts: u32,
    /// Base delay in milliseconds; actual delay is `min(base_delay_ms * 2^(attempt-1), max_delay_ms)`.
    pub base_delay_ms: u64,
    /// Hard ceiling on the computed delay in milliseconds.
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 5_000,
            max_delay_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Object-safe trait for downloading a single Spotify track's audio data.
///
/// Implementations handle:
/// - Resolving the best available OGG Vorbis `FileId` (with alternative-track fallback)
/// - Requesting the per-track audio decryption key
/// - Decrypting the audio stream
/// - Stripping Spotify's proprietary 167-byte OGG header
/// - Writing the result into a [`NamedTempFile`]
///
/// `download` is **synchronous** by design: it is meant to be called from
/// within `tokio::task::spawn_blocking` as per the queue architecture.
/// Implementations that need to drive async librespot calls should use
/// `tokio::runtime::Handle::current().block_on(...)`.
pub trait AudioFileDownloader: Send + Sync + 'static {
    fn download(&self, track_uri: &str) -> anyhow::Result<DownloadedTrack>;
}

// ---------------------------------------------------------------------------
// Shared helpers (pure, no librespot dependency — easy to unit-test)
// ---------------------------------------------------------------------------

/// Skip the first `header_len` bytes, then copy the rest from `reader` into `writer`.
///
/// Returns the number of content (non-header) bytes written.
///
/// Spotify prepends exactly 167 bytes (`0xa7`) of proprietary metadata to every
/// OGG Vorbis file. Standard players and lofty don't understand this prefix, so
/// it must be discarded before any further processing.
pub fn strip_header_and_copy<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    header_len: usize,
    buf_size: usize,
) -> anyhow::Result<usize> {
    // Discard the proprietary header
    let mut skip = vec![0u8; header_len];
    reader.read_exact(&mut skip)?;

    // Stream the remainder in fixed-size chunks
    let mut buf = vec![0u8; buf_size];
    let mut total = 0usize;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                writer.write_all(&buf[..n])?;
                total += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    writer.flush()?;
    Ok(total)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn strip_header_skips_exactly_header_bytes() {
        const HDR: usize = 167; // Spotify's 0xa7
        let mut data = vec![0xFFu8; HDR]; // header padding
        data.extend_from_slice(b"OggS real audio");
        let mut out = Vec::new();

        let n = strip_header_and_copy(&mut Cursor::new(data), &mut out, HDR, 4096).unwrap();

        assert_eq!(n, b"OggS real audio".len());
        assert_eq!(out, b"OggS real audio");
    }

    #[test]
    fn strip_header_with_only_header_data_produces_empty_output() {
        let data = vec![0u8; 167];
        let mut out = Vec::new();

        let n = strip_header_and_copy(&mut Cursor::new(data), &mut out, 167, 4096).unwrap();

        assert_eq!(n, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn strip_header_copies_multiple_buffer_sized_chunks() {
        const HDR: usize = 167;
        const BUF: usize = 256;
        let mut data = vec![0u8; HDR];
        data.extend(vec![0xABu8; BUF * 3 + 17]); // spans several chunks
        let mut out = Vec::new();

        let n = strip_header_and_copy(&mut Cursor::new(data), &mut out, HDR, BUF).unwrap();

        assert_eq!(n, BUF * 3 + 17);
        assert!(out.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn retry_config_default_matches_core_old_settings() {
        let cfg = RetryConfig::default();
        assert_eq!(cfg.max_attempts, 5);
        assert_eq!(cfg.base_delay_ms, 5_000);
        assert_eq!(cfg.max_delay_ms, 30_000);
    }
}
