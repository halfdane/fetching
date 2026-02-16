//! Audio playback functionality.
//!
//! Plays cached OGG Vorbis files through the system's audio output.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use rodio::{Decoder, OutputStream, Sink};
use tracing::info;

/// Play an OGG Vorbis file through the default audio output.
///
/// Blocks until playback is complete or an error occurs.
///
/// # Errors
///
/// Returns error if:
/// - File cannot be read or decoded
/// - Audio output device is unavailable
/// - Playback fails
pub fn play_audio_file(path: &Path) -> anyhow::Result<()> {
    let file = File::open(path)?;
    let source = Decoder::new(BufReader::new(file))?;
    
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    
    sink.append(source);
    sink.sleep_until_end();
    
    Ok(())
}

/// Play a list of audio files sequentially.
///
/// # Errors
///
/// Returns error if any file fails to play. Stops at first error.
pub fn play_audio_files(paths: &[impl AsRef<Path>]) -> anyhow::Result<()> {
    info!("Starting playback of {} tracks", paths.len());
    
    for (idx, path) in paths.iter().enumerate() {
        let path_ref = path.as_ref();
        info!("Playing track {}/{}: {}", idx + 1, paths.len(), path_ref.display());
        play_audio_file(path_ref)?;
    }
    
    info!("Playback complete");
    Ok(())
}
