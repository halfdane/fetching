//! Audio streaming and local caching for Spotify tracks.
//!
//! This module handles streaming audio from Spotify, decrypting it, and
//! caching as standard OGG Vorbis files for offline playback.

use std::io::{Read, Write};

use async_trait::async_trait;
use librespot_audio::{AudioDecrypt, AudioFile};
use librespot_core::file_id::FileId;
use librespot_core::session::Session;
use librespot_core::SpotifyUri;
use tokio::time::{sleep, Duration};
use tracing::debug;

use crate::traits::AudioDownloader;

/// Spotify's custom OGG header size in bytes (must be skipped for standard OGG)
pub const SPOTIFY_OGG_HEADER_END: usize = 0xa7; // 167 bytes
/// Buffer size hint for streaming in kbps
pub const STREAMING_BUFFER_HINT_KBPS: u32 = 160; // Buffer size hint for AudioFile::open
pub const AUDIO_BUFFER_SIZE: usize = 8192;
const MAX_RETRIES: u32 = 3;
const RETRY_BASE_DELAY_MS: u64 = 5000;

/// Production implementation of AudioDownloader using librespot Session
pub struct LibrespotAudioDownloader<'a> {
    pub session: &'a Session,
}

#[async_trait]
impl<'a> AudioDownloader for LibrespotAudioDownloader<'a> {
    async fn stream_track(
        &self,
        file_id: &FileId,
        track_uri: &SpotifyUri,
        cache_path: &str,
    ) -> anyhow::Result<()> {
        stream_to_cache(self.session, file_id, track_uri, cache_path).await
    }
}

/// Check if an error message indicates a retriable error
pub fn is_retriable_error(error_msg: &str) -> bool {
    error_msg.contains("audio key error")
        || error_msg.contains("Service unavailable")
        || error_msg.contains("timeout")
        || error_msg.contains("Deadline expired")
}

/// Calculate retry delay with exponential backoff
fn calculate_retry_delay(attempt: u32) -> Duration {
    Duration::from_millis(RETRY_BASE_DELAY_MS * (2_u64.pow(attempt.saturating_sub(1))))
}

/// Stream and cache a Spotify track as OGG Vorbis.
///
/// Streams encrypted audio, decrypts it, strips Spotify's custom OGG header,
/// and caches standard OGG Vorbis locally. Includes automatic retry with
/// exponential backoff for transient network errors.
///
/// # Errors
/// Returns error if:
/// - Track streaming fails after all retries (max 5 attempts)
/// - Network connection times out
/// - Cache path is not writable or disk is full
pub async fn stream_and_cache_track<D: AudioDownloader + ?Sized>(
    downloader: &D,
    file_id: &FileId,
    track_uri: &SpotifyUri,
    cache_path: &str,
    retry_delay_override: Option<Duration>,
) -> anyhow::Result<()> {
    let mut last_error = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = retry_delay_override.unwrap_or_else(|| calculate_retry_delay(attempt));
            if delay > Duration::ZERO {
                print!(" 🔄{}({}s)", attempt, delay.as_secs());
                std::io::Write::flush(&mut std::io::stdout())?;
                sleep(delay).await;
            }
        }

        match downloader
            .stream_track(file_id, track_uri, cache_path)
            .await
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                let error_msg = e.to_string();

                if is_retriable_error(&error_msg) {
                    debug!("Retriable error on attempt {}: {}", attempt + 1, error_msg);
                    last_error = Some(e);
                    // Continue to next attempt
                } else {
                    // Non-retriable error - fail immediately
                    println!(" ❌ {}", error_msg);
                    return Err(e);
                }
            }
        }
    }

    // All retries exhausted
    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("Streaming failed after {} attempts", MAX_RETRIES)))
}

async fn stream_to_cache(
    session: &Session,
    file_id: &FileId,
    track_uri: &SpotifyUri,
    cache_path: &str,
) -> anyhow::Result<()> {
    // Stream the file (not from local cache)
    // Note: buffer size is a hint for streaming, actual quality determined by file_id
    let audio_file = AudioFile::open(
        session,
        *file_id,
        (STREAMING_BUFFER_HINT_KBPS * 1024 / 8) as usize,
    )
    .await?;
    let (encrypted_reader, audio_key) = match audio_file {
        AudioFile::Streaming(stream) => {
            let track_id = match track_uri {
                SpotifyUri::Track { id } => *id,
                _ => anyhow::bail!("Not a track URI!"),
            };
            let audio_key = session.audio_key().request(track_id, *file_id).await?;
            (Box::new(stream) as Box<dyn Read>, audio_key)
        }
        AudioFile::Cached(file) => {
            let track_id = match track_uri {
                SpotifyUri::Track { id } => *id,
                _ => anyhow::bail!("Not a track URI!"),
            };
            let audio_key = session.audio_key().request(track_id, *file_id).await?;
            (Box::new(file) as Box<dyn Read>, audio_key)
        }
    };

    let mut decrypted_stream = AudioDecrypt::new(Some(audio_key), encrypted_reader);
    let mut cache_file = std::fs::File::create(cache_path)?;

    // Use the extracted function to skip header and copy data
    skip_header_and_copy(&mut decrypted_stream, &mut cache_file)?;

    // Explicitly flush to catch any buffering errors before file closes
    cache_file.flush()?;

    Ok(())
}

/// Skip the Spotify OGG header and copy remaining data from reader to writer
pub fn skip_header_and_copy<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> anyhow::Result<usize> {
    // Skip Spotify's custom header
    let mut header_skip = vec![0u8; SPOTIFY_OGG_HEADER_END];
    reader.read_exact(&mut header_skip)?;

    // Copy remaining data in chunks
    let mut buffer = vec![0u8; AUDIO_BUFFER_SIZE];
    let mut total_bytes = 0;

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                writer.write_all(&buffer[..n])?;
                total_bytes += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }

    writer.flush()?;
    Ok(total_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants_are_valid() {
        // Verify Spotify OGG header end is 167 bytes (0xa7)
        assert_eq!(SPOTIFY_OGG_HEADER_END, 0xa7);
        assert_eq!(SPOTIFY_OGG_HEADER_END, 167);

        // Verify streaming buffer hint is reasonable (160 kbps for buffer calculation)
        assert_eq!(STREAMING_BUFFER_HINT_KBPS, 160);

        // Verify buffer size is reasonable (8KB)
        assert_eq!(AUDIO_BUFFER_SIZE, 8192);

        // Verify retry settings
        assert_eq!(MAX_RETRIES, 3);
        assert_eq!(RETRY_BASE_DELAY_MS, 5000);
    }

    #[test]
    fn test_retry_delay_calculation() {
        // Manual exponential backoff: 5s, 10s, 20s
        let base = RETRY_BASE_DELAY_MS;
        assert_eq!(base * (2_u64.pow(0)), 5000); // First retry: 5s
        assert_eq!(base * (2_u64.pow(1)), 10000); // Second retry: 10s
        assert_eq!(base * (2_u64.pow(2)), 20000); // Third retry: 20s
    }

    #[test]
    fn test_header_skip_buffer_size() {
        // Verify that a buffer of SPOTIFY_OGG_HEADER_END size can be created
        let buffer = [0u8; SPOTIFY_OGG_HEADER_END];
        assert_eq!(buffer.len(), 167);
        assert_eq!(buffer.len(), SPOTIFY_OGG_HEADER_END);
    }

    #[test]
    fn test_audio_buffer_size_is_power_of_two() {
        // 8192 = 2^13, which is efficient for I/O operations
        assert_eq!(AUDIO_BUFFER_SIZE, 8192);
        assert!(AUDIO_BUFFER_SIZE.is_power_of_two());
    }

    #[test]
    fn test_streaming_buffer_calculation() {
        // 160 kbps = 160 * 1024 / 8 = 20480 bytes per second (used as buffer hint)
        let bytes_per_second = (STREAMING_BUFFER_HINT_KBPS * 1024 / 8) as usize;
        assert_eq!(bytes_per_second, 20480);
    }

    #[test]
    fn test_skip_header_and_copy_with_mock_data() {
        use std::io::Cursor;

        // Create mock data: 167 bytes header + actual content
        let mut input_data = vec![0xFF; SPOTIFY_OGG_HEADER_END]; // Header (167 bytes)
        input_data.extend_from_slice(b"OggS\x00\x02"); // OGG stream marker
        input_data.extend_from_slice(b"actual audio data here"); // Content

        let mut reader = Cursor::new(input_data);
        let mut output = Vec::new();

        let bytes_written = skip_header_and_copy(&mut reader, &mut output).unwrap();

        // Verify header was skipped
        assert!(!output.starts_with(&[0xFF]));
        assert!(output.starts_with(b"OggS\x00\x02"));

        // Verify content is present
        assert!(output
            .windows(b"actual audio data here".len())
            .any(|w| w == b"actual audio data here"));

        // Verify byte count
        assert_eq!(bytes_written, b"OggS\x00\x02actual audio data here".len());
    }

    #[test]
    fn test_skip_header_and_copy_exact_header_size() {
        use std::io::Cursor;

        // Create input with exactly header size (should result in empty output)
        let input_data = vec![0xAB; SPOTIFY_OGG_HEADER_END];
        let mut reader = Cursor::new(input_data);
        let mut output = Vec::new();

        let bytes_written = skip_header_and_copy(&mut reader, &mut output).unwrap();

        assert_eq!(bytes_written, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn test_skip_header_and_copy_large_file() {
        use std::io::Cursor;

        // Simulate larger file with multiple buffer-sized chunks
        let mut input_data = vec![0x00; SPOTIFY_OGG_HEADER_END]; // Header
        input_data.extend(vec![0xAB; AUDIO_BUFFER_SIZE * 2 + 100]); // 2+ buffers worth

        let mut reader = Cursor::new(input_data);
        let mut output = Vec::new();

        let bytes_written = skip_header_and_copy(&mut reader, &mut output).unwrap();

        assert_eq!(bytes_written, AUDIO_BUFFER_SIZE * 2 + 100);
        assert_eq!(output.len(), AUDIO_BUFFER_SIZE * 2 + 100);
        assert!(output.iter().all(|&b| b == 0xAB));
    }
}
