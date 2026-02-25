//! Output path construction for downloaded tracks.
//!
//! Builds paths of the form:
//! ```text
//! {base}/{primary_artist}/{year} - {album}/{02track} - {title}.{ext}
//! ```
//!
//! Every user-supplied segment is sanitised with [`sanitize_filename`] before
//! being pushed onto the [`PathBuf`], so strings like `"../etc/passwd"` or
//! Windows reserved names like `"CON"` can never escape the base directory.

use std::path::{Path, PathBuf};

use librespot_metadata::audio::AudioFileFormat;

use crate::container::{Track, TrackCollection};

// ---------------------------------------------------------------------------
// Format → file extension
// ---------------------------------------------------------------------------

/// Return the conventional file extension for a given Spotify audio format.
pub fn ext_for_format(fmt: AudioFileFormat) -> &'static str {
    match fmt {
        AudioFileFormat::OGG_VORBIS_320
        | AudioFileFormat::OGG_VORBIS_160
        | AudioFileFormat::OGG_VORBIS_96 => "ogg",

        AudioFileFormat::FLAC_FLAC | AudioFileFormat::FLAC_FLAC_24BIT => "flac",

        AudioFileFormat::MP3_320
        | AudioFileFormat::MP3_256
        | AudioFileFormat::MP3_160
        | AudioFileFormat::MP3_160_ENC
        | AudioFileFormat::MP3_96 => "mp3",

        // AAC / MP4 containers
        AudioFileFormat::AAC_320
        | AudioFileFormat::AAC_160
        | AudioFileFormat::AAC_48
        | AudioFileFormat::AAC_24
        | AudioFileFormat::MP4_128
        | AudioFileFormat::XHE_AAC_24
        | AudioFileFormat::XHE_AAC_16
        | AudioFileFormat::XHE_AAC_12 => "m4a",

        // Unknown format used by Spotify (~320 kbps)
        AudioFileFormat::OTHER5 => "bin",
    }
}

// ---------------------------------------------------------------------------
// Component sanitisation
// ---------------------------------------------------------------------------

/// Sanitise a single path component (file or directory name).
///
/// - Strips characters illegal on any major filesystem (`/`, `\`, `:`, `*`,
///   `?`, `"`, `<`, `>`, `|`, null bytes, control characters)
/// - Replaces Windows reserved names (`CON`, `NUL`, `COM1`, …)
/// - Trims the result to 200 characters to stay well within filesystem limits
/// - Returns `"_"` if the sanitised result is empty
pub fn safe_component(s: &str) -> String {
    let sanitized = sanitize_filename::sanitize(s);
    let trimmed = sanitized.trim();
    let truncated = if trimmed.len() > 200 {
        // Truncate at a char boundary
        &trimmed[..trimmed
            .char_indices()
            .take_while(|(i, _)| *i < 200)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0)]
    } else {
        trimmed
    };
    if truncated.is_empty() {
        "_".to_string()
    } else {
        truncated.to_string()
    }
}

// ---------------------------------------------------------------------------
// Path builder
// ---------------------------------------------------------------------------

/// Build the output path for a downloaded track.
///
/// # Structure
/// ```text
/// {base}/{primary_artist}/{year} - {album}/{02track} - {title}.{ext}
/// ```
///
/// For multi-disc releases where `disc_number` is `Some(d)` and `d > 1`, a
/// disc prefix is prepended to the track component:
/// ```text
/// {base}/{artist}/{year} - {album}/{disc}-{02track} - {title}.{ext}
/// ```
///
/// Every segment is sanitised with [`safe_component`] before being pushed,
/// so no field value can introduce path traversal.
pub fn build_output_path(
    base: &Path,
    track: &Track,
    collection: &TrackCollection,
    fmt: AudioFileFormat,
) -> PathBuf {
    let artist = safe_component(
        track.artists.first()
            .map(String::as_str)
            .unwrap_or("Unknown Artist"),
    );

    let year = collection
        .date
        .as_deref()
        .and_then(|d| d.get(..4))
        .unwrap_or("0000")
        .to_string();

    let album = safe_component(&collection.title);

    let track_component = match track.disc_number {
        Some(d) if d > 1 => format!(
            "{}-{:02} - {}.{}",
            d,
            track.number,
            safe_component(&track.title),
            ext_for_format(fmt),
        ),
        _ => format!(
            "{:02} - {}.{}",
            track.number,
            safe_component(&track.title),
            ext_for_format(fmt),
        ),
    };

    let mut path = base.to_path_buf();
    path.push(artist);
    path.push(format!("{year} - {album}"));
    path.push(track_component);
    path
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{CollectionType, TrackCollection, TrackType};
    use std::path::Path;

    fn make_collection(title: &str, date: Option<&str>) -> TrackCollection {
        TrackCollection {
            uri_str: "spotify:album:test".to_string(),
            spotify_id: "test".to_string(),
            collection_type: CollectionType::Album,
            title: title.to_string(),
            artists: vec!["Artist".to_string()],
            cover_id: None,
            upc: None,
            total_tracks: 1,
            popularity: None,
            label: None,
            date: date.map(str::to_string),
            track_uris: vec![],
        }
    }

    fn make_track(title: &str, artist: &str, number: i32, disc: Option<i32>) -> Track {
        Track {
            uri_str: "spotify:track:test".to_string(),
            spotify_id: "test".to_string(),
            track_type: TrackType::Track,
            title: title.to_string(),
            artists: vec![artist.to_string()],
            cover_id: None,
            isrc: None,
            duration_ms: 180_000,
            disc_number: disc,
            number,
            date: "2000".to_string(),
            popularity: None,
            explicit: false,
            language: vec![],
        }
    }

    #[test]
    fn normal_path_is_correct() {
        let col = make_collection("Flood", Some("1990-01-15"));
        let track = make_track("Birdhouse in Your Soul", "They Might Be Giants", 2, None);
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::OGG_VORBIS_320);
        assert_eq!(
            path,
            Path::new("/music/They Might Be Giants/1990 - Flood/02 - Birdhouse in Your Soul.ogg")
        );
    }

    #[test]
    fn primary_artist_only_not_all_artists() {
        let col = make_collection("Album", Some("2000"));
        let mut track = make_track("Track", "Primary Artist", 1, None);
        track.artists = vec!["Primary Artist".to_string(), "Featured Artist".to_string()];
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::OGG_VORBIS_320);
        assert!(path.to_str().unwrap().contains("Primary Artist"));
        assert!(!path.to_str().unwrap().contains("Featured Artist"));
    }

    #[test]
    fn traversal_attack_is_neutralised() {
        let col = make_collection("../etc", Some("2000"));
        let track = make_track("passwd", "../etc/passwd", 1, None);
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::OGG_VORBIS_320);
        let s = path.to_str().unwrap();
        assert!(!s.contains("/etc/passwd"), "traversal not neutralised: {s}");
        assert!(s.starts_with("/music/"), "base dir escaped: {s}");
    }

    #[test]
    fn windows_reserved_name_is_sanitised_on_windows_left_alone_on_linux() {
        // sanitize_filename replaces Windows reserved names (CON, NUL, COM1, …)
        // only when running on Windows. On Linux these are valid filenames.
        // The important guarantee is that the path stays under base and no
        // filesystem-illegal characters are introduced regardless of platform.
        let col = make_collection("CON", Some("2000"));
        let track = make_track("NUL", "CON", 1, None);
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::MP3_320);
        let s = path.to_str().unwrap();
        // Must always stay under base — no traversal regardless of OS
        assert!(s.starts_with("/music/"), "base dir escaped: {s}");
        #[cfg(windows)]
        assert!(!s.contains("/CON/"), "reserved name CON not sanitised on Windows: {s}");
    }

    #[test]
    fn multi_disc_prefixes_disc_number() {
        let col = make_collection("Double Album", Some("2005"));
        let track = make_track("Side B Opener", "Band", 1, Some(2));
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::FLAC_FLAC);
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with("2-01"));
    }

    #[test]
    fn disc_1_has_no_disc_prefix() {
        let col = make_collection("Album", Some("2005"));
        let track = make_track("Opener", "Band", 1, Some(1));
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::FLAC_FLAC);
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with("01"));
    }

    #[test]
    fn missing_date_falls_back_to_0000() {
        let col = make_collection("Dateless", None);
        let track = make_track("Track", "Artist", 1, None);
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::OGG_VORBIS_160);
        assert!(path.to_str().unwrap().contains("0000 - Dateless"));
    }

    #[test]
    fn empty_artist_falls_back_to_unknown() {
        let col = make_collection("Album", Some("2000"));
        let mut track = make_track("Track", "Artist", 1, None);
        track.artists = vec![];
        let path = build_output_path(Path::new("/music"), &track, &col, AudioFileFormat::OGG_VORBIS_320);
        assert!(path.to_str().unwrap().contains("Unknown Artist"));
    }

    #[test]
    fn ext_for_format_covers_all_variants() {
        let cases = [
            (AudioFileFormat::OGG_VORBIS_320, "ogg"),
            (AudioFileFormat::OGG_VORBIS_160, "ogg"),
            (AudioFileFormat::OGG_VORBIS_96,  "ogg"),
            (AudioFileFormat::FLAC_FLAC,       "flac"),
            (AudioFileFormat::FLAC_FLAC_24BIT, "flac"),
            (AudioFileFormat::MP3_320,         "mp3"),
            (AudioFileFormat::AAC_320,         "m4a"),
            (AudioFileFormat::MP4_128,         "m4a"),
            (AudioFileFormat::XHE_AAC_12,      "m4a"),
            (AudioFileFormat::OTHER5,          "bin"),
        ];
        for (fmt, expected) in cases {
            assert_eq!(ext_for_format(fmt), expected, "wrong ext for {fmt:?}");
        }
    }
}
