//! Librespot-backed implementation of [`AudioFileDownloader`].
//!
//! # What this does
//!
//! 1. Resolves the best available audio file for a track via `AudioItem`, which
//!    also handles **availability checking** (regional restrictions, embargoes,
//!    explicit-content filtering) that raw `Track::get` silently ignores.
//! 2. Tries the primary URI first; if it has no suitable files or is unavailable,
//!    fetches all per-market alternative URIs **in parallel** via `FuturesUnordered`
//!    and picks the best quality from all candidates combined.
//! 3. Selects the highest quality format across **all formats** (OGG, MP3, AAC,
//!    FLAC). Within the same quality tier OGG is preferred because it is Spotify's
//!    primary container and is what we know how to tag with lofty.
//! 4. Uses the selected format's actual data rate as the `AudioFile::open` buffer
//!    hint instead of a hardcoded 160 kbps value.
//! 5. Strips Spotify's proprietary 167-byte header **only for OGG Vorbis** files.
//!    MP3/AAC files do not carry this header and must not be truncated.
//! 6. Treats audio key failures as **survivable** (some content is unencrypted).
//!    Key errors no longer fire the retry loop; missing key → `AudioDecrypt(None)`.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{FuturesUnordered, StreamExt};
use librespot_audio::{AudioDecrypt, AudioFile};
use librespot_core::{file_id::FileId, session::Session, SpotifyUri};
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem};
use tempfile::NamedTempFile;
use tokio::runtime::Handle;
use tracing::{debug, info, warn};

use crate::audio::{
    is_retriable_error, strip_header_and_copy, AudioFileDownloader, DownloadedTrack, RetryConfig,
};

// ---------------------------------------------------------------------------
// Format quality table
// ---------------------------------------------------------------------------

/// All known Spotify audio formats ordered by descending quality.
///
/// Within the same perceptual quality tier, OGG Vorbis is listed first because:
/// - It is Spotify's primary container
/// - Its Vorbis Comments are the easiest target for lofty tagging
/// - The 167-byte header strip logic is well-understood for OGG
const FORMAT_PREFERENCE: &[(AudioFileFormat, u8)] = &[
    // — Lossless ————————————————————————————————————————
    (AudioFileFormat::FLAC_FLAC, 0),
    (AudioFileFormat::FLAC_FLAC_24BIT, 1),
    // — 320 kbps ————————————————————————————————————————
    (AudioFileFormat::OGG_VORBIS_320, 2),
    (AudioFileFormat::AAC_320, 3),
    (AudioFileFormat::MP3_320, 4),
    (AudioFileFormat::OTHER5, 5), // treated as ~320 by Spotify
    // — 256 kbps ————————————————————————————————————————
    (AudioFileFormat::MP3_256, 6),
    // — 160 kbps ————————————————————————————————————————
    (AudioFileFormat::OGG_VORBIS_160, 7),
    (AudioFileFormat::AAC_160, 8),
    (AudioFileFormat::MP3_160, 9),
    (AudioFileFormat::MP3_160_ENC, 10),
    (AudioFileFormat::MP4_128, 11), // ~128 kbps, slotted with the 160 tier
    // — 96 kbps ————————————————————————————————————————
    (AudioFileFormat::OGG_VORBIS_96, 12),
    (AudioFileFormat::MP3_96, 13),
    // — Low bitrate ————————————————————————————————————
    (AudioFileFormat::AAC_48, 14),
    (AudioFileFormat::AAC_24, 15),
    (AudioFileFormat::XHE_AAC_24, 16),
    (AudioFileFormat::XHE_AAC_16, 17),
    (AudioFileFormat::XHE_AAC_12, 18),
];

/// Quality rank for a format (lower = better). `u8::MAX` = unknown/unsupported.
fn format_rank(fmt: AudioFileFormat) -> u8 {
    FORMAT_PREFERENCE
        .iter()
        .find_map(|(f, rank)| if *f == fmt { Some(*rank) } else { None })
        .unwrap_or(u8::MAX)
}

/// Buffer hint bytes/sec for `AudioFile::open` — derived from the format's actual bitrate.
fn data_rate_bytes_per_sec(fmt: AudioFileFormat) -> usize {
    let kbps: f32 = match fmt {
        AudioFileFormat::FLAC_FLAC | AudioFileFormat::FLAC_FLAC_24BIT => 112.0,
        AudioFileFormat::OGG_VORBIS_320
        | AudioFileFormat::AAC_320
        | AudioFileFormat::MP3_320
        | AudioFileFormat::OTHER5 => 40.0,
        AudioFileFormat::MP3_256 => 32.0,
        AudioFileFormat::OGG_VORBIS_160
        | AudioFileFormat::AAC_160
        | AudioFileFormat::MP3_160
        | AudioFileFormat::MP3_160_ENC => 20.0,
        AudioFileFormat::MP4_128 => 16.0,
        AudioFileFormat::OGG_VORBIS_96 | AudioFileFormat::MP3_96 => 12.0,
        AudioFileFormat::AAC_48 => 6.0,
        AudioFileFormat::AAC_24 | AudioFileFormat::XHE_AAC_24 => 3.0,
        AudioFileFormat::XHE_AAC_16 => 2.0,
        AudioFileFormat::XHE_AAC_12 => 1.5,
    };
    (kbps * 1024.0).ceil() as usize
}

/// Pick the best available format from an `AudioFiles` map.
fn best_format(files: &AudioFiles) -> Option<(FileId, AudioFileFormat)> {
    FORMAT_PREFERENCE
        .iter()
        .find_map(|(fmt, _)| files.0.get(fmt).copied().map(|id| (id, *fmt)))
}

// ---------------------------------------------------------------------------
// Public struct
// ---------------------------------------------------------------------------

pub struct LibrespotAudioDownloader {
    pub session: Arc<Session>,
    pub retry_config: RetryConfig,
}

impl LibrespotAudioDownloader {
    pub fn new(session: Arc<Session>) -> Self {
        Self {
            session,
            retry_config: RetryConfig::default(),
        }
    }

    pub fn with_retry(session: Arc<Session>, retry_config: RetryConfig) -> Self {
        Self {
            session,
            retry_config,
        }
    }
}

// ---------------------------------------------------------------------------
// AudioFileDownloader impl
// ---------------------------------------------------------------------------

impl AudioFileDownloader for LibrespotAudioDownloader {
    fn download(&self, track_uri: &str) -> anyhow::Result<DownloadedTrack> {
        let handle = Handle::current();

        // Resolve the best (file_id, format, owning_uri) — may try alternatives.
        let (file_id, format, resolved_uri) =
            handle.block_on(resolve_best_audio(&self.session, track_uri))?;

        debug!("Resolved {:?} for {} via {}", format, track_uri, resolved_uri);

        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..self.retry_config.max_attempts {
            if attempt > 0 {
                let delay = Duration::from_millis(
                    self.retry_config.base_delay_ms * 2u64.pow(attempt - 1),
                );
                info!(
                    "Retrying audio download for {} (attempt {}/{}) after: {}",
                    track_uri,
                    attempt + 1,
                    self.retry_config.max_attempts,
                    last_err.as_ref().map(|e| e.to_string()).unwrap_or_default()
                );
                std::thread::sleep(delay);
            }

            match stream_to_tempfile(
                &self.session,
                &file_id,
                format,
                &resolved_uri,
                track_uri,
                &handle,
            ) {
                Ok(downloaded) => return Ok(downloaded),
                Err(e) => {
                    let msg = e.to_string();
                    if is_retriable_error(&msg) {
                        warn!("Retriable error on attempt {}: {}", attempt + 1, msg);
                        last_err = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!(
                "Audio download for {} failed after {} attempts",
                track_uri,
                self.retry_config.max_attempts
            )
        }))
    }
}

// ---------------------------------------------------------------------------
// Audio resolution: AudioItem + parallel alternatives
// ---------------------------------------------------------------------------

/// Resolve the best `(FileId, AudioFileFormat, owning_uri)` for `track_uri`.
///
/// Uses `AudioItem::get_file` which checks regional availability, embargo dates,
/// and explicit-content filters — none of which `Track::get` handles.
///
/// If the primary URI is unavailable or has no suitable formats, all per-market
/// alternatives are fetched **in parallel** via `FuturesUnordered`. The best
/// quality across all available candidates is selected.
///
/// Fast path: if the primary URI already has lossless FLAC (rank 0),
/// alternatives are skipped entirely.
async fn resolve_best_audio(
    session: &Session,
    track_uri: &str,
) -> anyhow::Result<(FileId, AudioFileFormat, SpotifyUri)> {
    let uri = SpotifyUri::from_uri(track_uri)?;

    let primary = AudioItem::get_file(session, uri.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load AudioItem for {track_uri}: {e}"))?;

    type Candidate = (FileId, AudioFileFormat, SpotifyUri);
    let mut candidates: Vec<Candidate> = Vec::new();

    if primary.availability.is_ok() {
        if let Some((file_id, fmt)) = best_format(&primary.files) {
            // Fast path: lossless — no need to check alternatives
            if format_rank(fmt) == 0 {
                debug!("Primary track has lossless audio, skipping alternatives");
                return Ok((file_id, fmt, uri));
            }
            candidates.push((file_id, fmt, uri.clone()));
        }
    } else {
        debug!("Primary track unavailable: {:?}", primary.availability.as_ref().err());
    }

    // Fetch all alternatives in parallel
    if let Some(alternatives) = primary.alternatives {
        let futures: FuturesUnordered<_> = alternatives
            .iter()
            .map(|alt_uri| AudioItem::get_file(session, alt_uri.clone()))
            .collect();

        let mut stream = futures;
        while let Some(result) = stream.next().await {
            match result {
                Ok(item) if item.availability.is_ok() => {
                    if let Some((file_id, fmt)) = best_format(&item.files) {
                        let owning_uri = item.track_id.clone();
                        candidates.push((file_id, fmt, owning_uri));
                        if format_rank(fmt) == 0 {
                            break; // lossless found — nothing better possible
                        }
                    }
                }
                Ok(item) => debug!("Alternative unavailable: {:?}", item.availability.err()),
                Err(e) => debug!("Alternative fetch failed: {e}"),
            }
        }
    }

    candidates.sort_by_key(|(_, fmt, _)| format_rank(*fmt));

    candidates
        .into_iter()
        .next()
        .map(|(file_id, fmt, uri)| {
            info!("Selected {:?} for {}", fmt, track_uri);
            (file_id, fmt, uri)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No suitable audio format found for {track_uri} (tried primary + alternatives)"
            )
        })
}

// ---------------------------------------------------------------------------
// Stream → AudioDecrypt → strip header (OGG only) → NamedTempFile
// ---------------------------------------------------------------------------

fn stream_to_tempfile(
    session: &Session,
    file_id: &FileId,
    format: AudioFileFormat,
    track_uri: &SpotifyUri,
    original_uri_str: &str,
    handle: &Handle,
) -> anyhow::Result<DownloadedTrack> {
    // Buffer hint is derived from the actual format bitrate.
    let buf_hint = data_rate_bytes_per_sec(format);

    let audio_file = handle.block_on(AudioFile::open(session, *file_id, buf_hint))?;

    let spotify_id = match track_uri {
        SpotifyUri::Track { id } => *id,
        SpotifyUri::Episode { id } => *id,
        _ => anyhow::bail!("Cannot request audio key for URI: {}", track_uri),
    };

    // Audio key failure is survivable: some content (previews, podcasts) is
    // unencrypted. Pass None to AudioDecrypt instead of failing the whole download.
    let key = match handle.block_on(session.audio_key().request(spotify_id, *file_id)) {
        Ok(k) => {
            debug!("Audio key obtained for {original_uri_str}");
            Some(k)
        }
        Err(e) => {
            warn!("Audio key request failed for {original_uri_str}, proceeding without decryption: {e}");
            None
        }
    };

    let raw_reader: Box<dyn std::io::Read> = match audio_file {
        AudioFile::Streaming(stream) => Box::new(stream),
        AudioFile::Cached(file) => Box::new(file),
    };

    let mut decrypted = AudioDecrypt::new(key, raw_reader);
    let mut temp_file = NamedTempFile::new()?;

    // The 167-byte Spotify header exists ONLY in OGG Vorbis files.
    // MP3, AAC, and FLAC start with their own native headers at byte 0.
    //
    // TODO(lofty): Before skipping, parse the header into a ReplayGain struct
    // and store it in DownloadedTrack so the lofty tagging step can write it
    // back as standard Vorbis Comments (REPLAYGAIN_TRACK_GAIN, REPLAYGAIN_TRACK_PEAK,
    // REPLAYGAIN_ALBUM_GAIN, REPLAYGAIN_ALBUM_PEAK). The byte layout is documented
    // in librespot-playback/src/normalisation.rs (NormalisationData — four f32, big-endian).
    let header_len = if AudioFiles::is_ogg_vorbis(format) { 0xa7 } else { 0 };

    let bytes = strip_header_and_copy(
        &mut decrypted,
        temp_file.as_file_mut(),
        header_len,
        8_192,
    )?;

    info!("Downloaded {bytes} bytes ({format:?}) for {original_uri_str}");

    Ok(DownloadedTrack {
        track_uri: original_uri_str.to_string(),
        file: temp_file,
    })
}
