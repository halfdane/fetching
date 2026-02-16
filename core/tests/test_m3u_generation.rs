use spotify_player_core::m3u::{write_m3u_playlist, M3uEntry};
use tempfile::TempDir;

#[test]
fn test_m3u_playlist_basic() {
    let temp_dir = TempDir::new().unwrap();
    let playlist_path = temp_dir.path().join("test.m3u8");

    let entries = vec![
        M3uEntry {
            duration: 180,
            artist: "Artist One".to_string(),
            title: "Song One".to_string(),
            file_path: temp_dir.path().join("Artist One/Album/Song One.ogg"),
        },
        M3uEntry {
            duration: 240,
            artist: "Artist Two".to_string(),
            title: "Song Two".to_string(),
            file_path: temp_dir.path().join("Artist Two/Album/Song Two.ogg"),
        },
    ];

    write_m3u_playlist(&playlist_path, &entries, None).unwrap();

    assert!(playlist_path.exists());
    let content = std::fs::read_to_string(&playlist_path).unwrap();

    assert!(content.contains("#EXTM3U"));
    assert!(content.contains("Artist One - Song One"));
    assert!(content.contains("Artist Two - Song Two"));
    assert!(content.contains("#EXTINF:180"));
    assert!(content.contains("#EXTINF:240"));
}

#[test]
fn test_m3u_playlist_with_spotify_url() {
    let temp_dir = TempDir::new().unwrap();
    let playlist_path = temp_dir.path().join("test.m3u8");

    let entries = vec![M3uEntry {
        duration: 180,
        artist: "Test Artist".to_string(),
        title: "Test Song".to_string(),
        file_path: temp_dir.path().join("test.ogg"),
    }];

    let spotify_url = Some("spotify:playlist:37i9dQZF1DX0XUsuxWHRQd");
    write_m3u_playlist(&playlist_path, &entries, spotify_url).unwrap();

    let content = std::fs::read_to_string(&playlist_path).unwrap();
    assert!(content.contains("# Source: spotify:playlist:37i9dQZF1DX0XUsuxWHRQd"));
}

#[test]
fn test_m3u_relative_paths() {
    let temp_dir = TempDir::new().unwrap();
    let playlists_dir = temp_dir.path().join("Playlists/MyPlaylist");
    std::fs::create_dir_all(&playlists_dir).unwrap();

    let playlist_path = playlists_dir.join("playlist.m3u8");
    let music_path = temp_dir.path().join("Music/Artist/Album/song.ogg");

    let entries = vec![M3uEntry {
        duration: 200,
        artist: "Artist".to_string(),
        title: "Song".to_string(),
        file_path: music_path,
    }];

    write_m3u_playlist(&playlist_path, &entries, None).unwrap();

    let content = std::fs::read_to_string(&playlist_path).unwrap();
    // Should use relative path, not absolute
    assert!(content.contains("../"));
}

#[test]
fn test_m3u_empty_entries() {
    let temp_dir = TempDir::new().unwrap();
    let playlist_path = temp_dir.path().join("empty.m3u8");

    let entries: Vec<M3uEntry> = vec![];
    write_m3u_playlist(&playlist_path, &entries, None).unwrap();

    let content = std::fs::read_to_string(&playlist_path).unwrap();
    assert!(content.contains("#EXTM3U"));
    // Should only have header
    assert_eq!(content.lines().count(), 1);
}
