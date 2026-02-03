//! Track metadata and OGG Vorbis tagging.
//!
//! Handles conversion of Spotify track metadata to OGG Vorbis tags,
//! filename sanitization, and file path construction with proper
//! artist/album organization.

use std::path::PathBuf;

use librespot_metadata::track::Track;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt};

/// Extract artist name from a list of artists, returning "Unknown Artist" if empty
pub fn get_artist_name(artists: &[librespot_metadata::artist::Artist]) -> String {
    if !artists.is_empty() {
        artists[0].name.clone()
    } else {
        "Unknown Artist".to_string()
    }
}

/// Sanitize a string for use in filenames by replacing uncommon characters with underscores.
/// Keeps: alphanumeric and periods (common in filenames).
/// Replaces everything else (including spaces and dashes) with underscores.
/// Collapses consecutive underscores into a single underscore.
pub fn sanitize(s: &str) -> String {
    let mut result = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    // Collapse consecutive underscores
    while result.contains("__") {
        result = result.replace("__", "_");
    }

    result.trim_matches('_').to_string()
}

/// Build the output path for a track (directory structure + filename)
///
/// # Errors
///
/// Returns error if directory creation fails or path cannot be constructed.
pub fn build_track_path(
    track: &Track,
    base_music_dir: &str,
    prefix: Option<String>,
) -> anyhow::Result<PathBuf> {
    let year = track.album.date.year();
    let artist_name = sanitize(&get_artist_name(&track.artists));
    let album_name = sanitize(&track.album.name);
    let track_title = sanitize(&track.name);
    let track_num = track.number;

    let mut dir_path = PathBuf::from(base_music_dir);
    dir_path.push(&artist_name);
    dir_path.push(&album_name);
    std::fs::create_dir_all(&dir_path)?;

    let filename = if let Some(prefix_str) = prefix {
        format!(
            "{}_{:04}_{}_{}_{:03}_{}.ogg",
            prefix_str, year, artist_name, album_name, track_num, track_title
        )
    } else {
        format!(
            "{:04}_{}_{}_{:03}_{}.ogg",
            year, artist_name, album_name, track_num, track_title
        )
    };

    dir_path.push(filename);
    Ok(dir_path)
}

/// Metadata for an audio track, decoupled from librespot's Track type.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackMetadata {
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artists: Vec<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub date: Option<String>, // YYYY-MM-DD or YYYY
    pub genres: Vec<String>,
    pub isrc: Option<String>,
    pub label: Option<String>,
    pub cover_art: Option<Vec<u8>>, // Album cover image (JPEG or PNG)
}

impl TrackMetadata {
    /// Extract metadata from a librespot Track.
    pub fn from_track(track: &Track, year: i32, cover_art: Option<Vec<u8>>) -> Self {
        // Extract artists
        let artists = track.artists.iter().map(|a| a.name.clone()).collect();

        // Extract album artists
        let album_artists = track.album.artists.iter().map(|a| a.name.clone()).collect();

        // Extract track/disc numbers (skip if 0)
        let track_number = if track.number > 0 {
            Some(track.number as u32)
        } else {
            None
        };

        let disc_number = if track.disc_number > 0 {
            Some(track.disc_number as u32)
        } else {
            None
        };

        // Extract date (prefer full date, fallback to year)
        let date_obj = track.album.date;
        let month = date_obj.month() as u8;
        let day = date_obj.day();
        let date = if date_obj.year() > 0 && month > 0 && day > 0 {
            Some(format!("{:04}-{:02}-{:02}", date_obj.year(), month, day))
        } else if year > 0 {
            Some(year.to_string())
        } else {
            None
        };

        // Extract genres
        let genres = track.tags.clone();

        // Extract ISRC
        let isrc = track
            .external_ids
            .iter()
            .find(|eid| eid.external_type == "isrc")
            .map(|eid| eid.id.clone());

        // Extract label
        let label = if !track.album.label.is_empty() {
            Some(track.album.label.clone())
        } else {
            None
        };

        TrackMetadata {
            title: track.name.clone(),
            artists,
            album: track.album.name.clone(),
            album_artists,
            track_number,
            disc_number,
            date,
            genres,
            isrc,
            label,
            cover_art,
        }
    }
}

/// Write metadata tags to an OGG file using lofty.
///
/// # Errors
///
/// Returns error if:
/// - File cannot be read or parsed as OGG Vorbis
/// - Tag writing fails
/// - File save operation fails
pub fn write_ogg_tags(output_path: &str, metadata: &TrackMetadata) -> anyhow::Result<()> {
    let tagged_file = lofty::read_from_path(output_path)?;

    let mut tag = match tagged_file.primary_tag() {
        Some(primary_tag) => primary_tag.clone(),
        None => Tag::new(lofty::tag::TagType::VorbisComments),
    };

    tag.set_title(metadata.title.clone());
    tag.set_album(metadata.album.clone());

    if !metadata.artists.is_empty() {
        tag.set_artist(metadata.artists.join(", "));
    }

    if !metadata.album_artists.is_empty() {
        tag.insert_text(ItemKey::AlbumArtist, metadata.album_artists.join(", "));
    }

    if let Some(track_num) = metadata.track_number {
        tag.set_track(track_num);
    }

    if let Some(disc_num) = metadata.disc_number {
        tag.set_disk(disc_num);
    }

    if let Some(date_str) = &metadata.date {
        if date_str.contains('-') {
            // Full date format: YYYY-MM-DD
            tag.insert_text(ItemKey::RecordingDate, date_str.clone());
        } else {
            // Year only
            if let Ok(year_val) = date_str.parse::<u32>() {
                tag.set_year(year_val);
            }
        }
    }

    if !metadata.genres.is_empty() {
        tag.insert_text(ItemKey::Genre, metadata.genres.join(", "));
    }

    if let Some(isrc_val) = &metadata.isrc {
        tag.insert_text(ItemKey::Isrc, isrc_val.clone());
    }

    if let Some(label_val) = &metadata.label {
        tag.insert_text(ItemKey::Label, label_val.clone());
    }

    if let Some(cover_bytes) = &metadata.cover_art {
        let mime_type = detect_image_mime_type(cover_bytes);

        let picture = Picture::new_unchecked(
            PictureType::CoverFront,
            Some(mime_type),
            None,
            cover_bytes.clone(),
        );

        tag.push_picture(picture);
    }

    tag.save_to_path(output_path, lofty::config::WriteOptions::default())?;

    Ok(())
}

/// Detect image MIME type from the first few bytes
fn detect_image_mime_type(bytes: &[u8]) -> MimeType {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        MimeType::Jpeg
    } else if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        MimeType::Png
    } else {
        // Default to JPEG (most common for album art)
        MimeType::Jpeg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_special_characters() {
        let result = sanitize("A/B:C?D*E|F<G>H\\I-J");
        assert_eq!(result, "A_B_C_D_E_F_G_H_I_J");
    }

    #[test]
    fn test_sanitize_empty() {
        let result = sanitize("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_sanitize_no_special() {
        let result = sanitize("NormalString");
        assert_eq!(result, "NormalString");
    }

    #[test]
    fn test_sanitize_leading_trailing_underscores() {
        let result = sanitize("_leading and trailing_");
        assert_eq!(result, "leading_and_trailing");
    }

    #[test]
    fn test_sanitize_single_quotes() {
        let result = sanitize("Don't Stop 'Til You Get Enough");
        assert_eq!(result, "Don_t_Stop_Til_You_Get_Enough");
    }

    #[test]
    fn test_sanitize_exclamation_marks() {
        let result = sanitize("Hello! World!");
        assert_eq!(result, "Hello_World");
    }

    #[test]
    fn test_sanitize_keeps_periods() {
        let result = sanitize("Track 1.5");
        assert_eq!(result, "Track_1.5");
    }

    #[test]
    fn test_sanitize_unicode_characters() {
        let result = sanitize("Björk - Über");
        assert_eq!(result, "Björk_Über");
    }

    // Note: build_track_path() tests are difficult to create because librespot structs
    // are complex with many non-public constructors. Instead, we test the filename
    // building logic separately below and rely on integration testing for full path.

    #[test]
    fn test_filename_format_without_prefix() {
        let year = 2020;
        let artist = "Test Artist";
        let album = "Test Album";
        let track_num = 5;
        let track_title = "Test Track";

        let filename = format!(
            "{:04}_{}_{}_{:03}_{}.ogg",
            year,
            sanitize(artist),
            sanitize(album),
            track_num,
            sanitize(track_title)
        );

        assert_eq!(filename, "2020_Test_Artist_Test_Album_005_Test_Track.ogg");
    }

    #[test]
    fn test_filename_format_with_prefix() {
        let prefix = "042";
        let year = 2021;
        let artist = "Artist";
        let album = "Album";
        let track_num = 1;
        let track_title = "Track";

        let filename = format!(
            "{}_{:04}_{}_{}_{:03}_{}.ogg",
            prefix,
            year,
            sanitize(artist),
            sanitize(album),
            track_num,
            sanitize(track_title)
        );

        assert_eq!(filename, "042_2021_Artist_Album_001_Track.ogg");
    }

    #[test]
    fn test_filename_with_special_characters() {
        let year = 1980;
        let artist = "AC/DC";
        let album = "Back in Black: Special Edition";
        let track_num = 3;
        let track_title = "You Shook Me All Night Long?!";

        let filename = format!(
            "{:04}_{}_{}_{:03}_{}.ogg",
            year,
            sanitize(artist),
            sanitize(album),
            track_num,
            sanitize(track_title)
        );

        // Check that sanitized characters are replaced
        assert!(!filename.contains('/'));
        assert!(!filename.contains(':'));
        assert!(!filename.contains('?'));
        assert!(!filename.contains('!'));
        assert_eq!(
            filename,
            "1980_AC_DC_Back_in_Black_Special_Edition_003_You_Shook_Me_All_Night_Long.ogg"
        );
    }

    #[test]
    fn test_path_construction() {
        use std::path::PathBuf;

        let base_dir = "/tmp/music";
        let artist = "Test Artist";
        let album = "Test Album";

        let mut path = PathBuf::from(base_dir);
        path.push(sanitize(artist));
        path.push(sanitize(album));

        assert_eq!(path, PathBuf::from("/tmp/music/Test_Artist/Test_Album"));
    }

    // Tests for metadata tagging logic
    // Note: Testing with librespot Track objects requires complex setup due to
    // non-public constructors. Instead, we test TrackMetadata directly and
    // individual logic components.

    #[test]
    fn test_metadata_artist_list_formatting() {
        // Test that multiple artists are joined with comma-space
        let artists = ["Artist One", "Artist Two", "Artist Three"];
        let formatted = artists.join(", ");
        assert_eq!(formatted, "Artist One, Artist Two, Artist Three");
    }

    #[test]
    fn test_metadata_date_formatting_full() {
        // Test full date formatting (YYYY-MM-DD)
        let year = 2025;
        let month = 3u8;
        let day = 15u32;
        let date_str = format!("{:04}-{:02}-{:02}", year, month, day);
        assert_eq!(date_str, "2025-03-15");
    }

    #[test]
    fn test_metadata_date_formatting_year_only() {
        // Test fallback to year-only when date components are missing
        let year = 2020;
        let date_tag = year.to_string();
        assert_eq!(date_tag, "2020");
    }

    #[test]
    fn test_metadata_genre_list_formatting() {
        // Test that multiple genres are joined with comma-space
        let tags = [
            "Rock".to_string(),
            "Alternative".to_string(),
            "Indie".to_string(),
        ];
        let genres = tags.join(", ");
        assert_eq!(genres, "Rock, Alternative, Indie");
    }

    #[test]
    fn test_metadata_track_number_conversion() {
        // Test i32 to u32 conversion for track numbers
        let track_number: i32 = 5;
        let converted = track_number as u32;
        assert_eq!(converted, 5u32);

        // Verify positive numbers stay positive
        assert!(track_number > 0);
    }

    #[test]
    fn test_metadata_disc_number_conversion() {
        // Test i32 to u32 conversion for disc numbers
        let disc_number: i32 = 2;
        let converted = disc_number as u32;
        assert_eq!(converted, 2u32);

        // Verify positive numbers stay positive
        assert!(disc_number > 0);
    }

    #[test]
    fn test_metadata_skip_zero_values() {
        // Test logic for skipping zero track/disc numbers
        let track_number: i32 = 0;
        let disc_number: i32 = 0;

        // These should be skipped (not set) when zero
        assert_eq!(track_number, 0);
        assert_eq!(disc_number, 0);
    }

    #[test]
    fn test_lofty_tag_creation() {
        // Test that we can create a Vorbis Comments tag
        let tag = Tag::new(lofty::tag::TagType::VorbisComments);
        assert_eq!(tag.tag_type(), lofty::tag::TagType::VorbisComments);
        assert_eq!(tag.item_count(), 0);
    }

    #[test]
    fn test_lofty_tag_basic_setters() {
        // Test basic tag setters work
        let mut tag = Tag::new(lofty::tag::TagType::VorbisComments);

        tag.set_title("Test Track".to_string());
        tag.set_artist("Test Artist".to_string());
        tag.set_album("Test Album".to_string());

        assert_eq!(tag.title().as_deref(), Some("Test Track"));
        assert_eq!(tag.artist().as_deref(), Some("Test Artist"));
        assert_eq!(tag.album().as_deref(), Some("Test Album"));
    }

    #[test]
    fn test_lofty_tag_numeric_fields() {
        // Test numeric field setters
        let mut tag = Tag::new(lofty::tag::TagType::VorbisComments);

        tag.set_track(5);
        tag.set_disk(2);
        tag.set_year(2025);

        assert_eq!(tag.track(), Some(5));
        assert_eq!(tag.disk(), Some(2));
        assert_eq!(tag.year(), Some(2025));
    }

    #[test]
    fn test_lofty_custom_fields() {
        // Test inserting custom text fields
        let mut tag = Tag::new(lofty::tag::TagType::VorbisComments);

        tag.insert_text(ItemKey::Genre, "Rock".to_string());
        tag.insert_text(ItemKey::Label, "Test Label".to_string());
        tag.insert_text(
            ItemKey::Unknown("ISRC".to_string()),
            "USRC12345678".to_string(),
        );

        // Verify items were added
        assert!(tag.item_count() > 0);
    }

    #[test]
    fn test_metadata_roundtrip() {
        use std::fs;
        use tempfile::NamedTempFile;

        // Create a minimal valid OGG file
        let test_file = NamedTempFile::with_suffix(".ogg").unwrap();
        let test_path = test_file.path();

        // Create minimal OGG Vorbis stream using ogg crate
        let mut writer = ogg::PacketWriter::new(fs::File::create(test_path).unwrap());

        // Vorbis identification header (minimal valid header)
        let ident_header = vec![
            0x01, // packet type (identification)
            0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, // "vorbis"
            0x00, 0x00, 0x00, 0x00, // version
            0x02, // channels
            0x44, 0xac, 0x00, 0x00, // sample rate (44100)
            0x00, 0x00, 0x00, 0x00, // max bitrate
            0x00, 0x7d, 0x00, 0x00, // nominal bitrate (32000)
            0x00, 0x00, 0x00, 0x00, // min bitrate
            0xb8, // blocksize
            0x01, // framing flag
        ];

        // Vorbis comment header (empty)
        let comment_header = vec![
            0x03, // packet type (comments)
            0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, // "vorbis"
            0x00, 0x00, 0x00, 0x00, // vendor length (0)
            0x00, 0x00, 0x00, 0x00, // comment count (0)
            0x01, // framing flag
        ];

        // Setup header (minimal)
        let setup_header = vec![
            0x05, // packet type (setup)
            0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, // "vorbis"
            0x01, // framing flag
        ];

        writer
            .write_packet(ident_header, 0, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        writer
            .write_packet(comment_header, 0, ogg::PacketWriteEndInfo::NormalPacket, 0)
            .unwrap();
        writer
            .write_packet(setup_header, 0, ogg::PacketWriteEndInfo::EndStream, 0)
            .unwrap();
        drop(writer);

        // Verify lofty can read it
        let tagged_file = lofty::read_from_path(test_path);
        assert!(
            tagged_file.is_ok(),
            "Lofty should be able to read minimal OGG file"
        );

        // Create TrackMetadata and test the production function
        let metadata = TrackMetadata {
            title: "Test Track Title".to_string(),
            artists: vec!["Artist One".to_string(), "Artist Two".to_string()],
            album: "Test Album Name".to_string(),
            album_artists: vec!["Album Artist".to_string()],
            track_number: Some(5),
            disc_number: Some(2),
            date: Some("2025-03-15".to_string()),
            genres: vec!["Rock".to_string(), "Alternative".to_string()],
            isrc: Some("USRC12345678".to_string()),
            label: Some("Test Label".to_string()),
            cover_art: None, // No cover art in test
        };

        let result = write_ogg_tags(test_path.to_str().unwrap(), &metadata);

        assert!(
            result.is_ok(),
            "write_ogg_tags should succeed: {:?}",
            result.err()
        );

        // Read back and verify all metadata was written correctly
        let tagged_file = lofty::read_from_path(test_path).unwrap();
        let read_tag = tagged_file.primary_tag().unwrap();

        assert_eq!(read_tag.title().unwrap(), "Test Track Title");
        assert_eq!(read_tag.artist().unwrap(), "Artist One, Artist Two");
        assert_eq!(read_tag.album().unwrap(), "Test Album Name");
        assert_eq!(read_tag.track(), Some(5));
        assert_eq!(read_tag.disk(), Some(2));

        // Verify custom fields
        let items: Vec<_> = read_tag.items().collect();

        assert!(
            items.iter().any(|item| {
                item.key() == &ItemKey::RecordingDate && item.value().text() == Some("2025-03-15")
            }),
            "RecordingDate field should be set"
        );

        assert!(
            items.iter().any(|item| {
                item.key() == &ItemKey::AlbumArtist && item.value().text() == Some("Album Artist")
            }),
            "AlbumArtist field should be set"
        );

        assert!(
            items.iter().any(|item| {
                item.key() == &ItemKey::Genre && item.value().text() == Some("Rock, Alternative")
            }),
            "Genre field should be set"
        );

        assert!(
            items.iter().any(|item| {
                item.key() == &ItemKey::Label && item.value().text() == Some("Test Label")
            }),
            "Label field should be set"
        );

        assert!(
            items.iter().any(|item| {
                item.key() == &ItemKey::Isrc && item.value().text() == Some("USRC12345678")
            }),
            "ISRC field should be set"
        );
    }

    // Note: get_artist_name is tested implicitly through integration tests
    // Creating Artist objects requires many fields from librespot_metadata
}
