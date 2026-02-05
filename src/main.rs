use std::env;

use anyhow::Context;
use librespot_core::SpotifyUri;
use tracing::{error, info};

mod auth;
mod cache;
mod config;
mod error;
mod m3u;
mod metadata;
mod playback;
mod stream;
mod traits;
mod implementations;

use auth::create_session_with_auto_refresh;
use cache::{cache_album, cache_playlist, process_track_cache};
use config::Config;
use metadata::build_track_path;
use implementations::LibrespotTrackFetcher;

/// Source of Spotify URIs to process
#[derive(Debug)]
pub enum InputSource {
    SingleUri(String),
    File(std::path::PathBuf),
}

/// Read Spotify URIs from a file (one per line)
fn read_uris_from_file(path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    
    let uris: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect();
    
    if uris.is_empty() {
        anyhow::bail!("No valid URIs found in file: {}", path.display());
    }
    
    Ok(uris)
}

/// Process a single Spotify URI (track, album, or playlist)
async fn process_single_uri(
    session: &librespot_core::session::Session,
    spotify_uri: &SpotifyUri,
    config: &Config,
    no_play: bool,
) -> anyhow::Result<()> {
    match spotify_uri {
        SpotifyUri::Track { .. } => {
            info!("Caching single track...");
            let track_fetcher = LibrespotTrackFetcher { session };
            let (track_provider, file_id) = cache::get_track_with_ogg_format(&track_fetcher, spotify_uri).await?;
            
            let track_display = format!("Track: {}", track_provider.name().await);
            print!("{}", track_display);
            std::io::Write::flush(&mut std::io::stdout())?;

            let music_dir = config.get_music_dir().map_err(|e| anyhow::anyhow!(e))?;
            let music_dir_str = music_dir.to_str().ok_or_else(|| {
                anyhow::anyhow!(error::DownloadError::InvalidUtf8Path(music_dir.clone()))
            })?;
            let output_path = build_track_path(&*track_provider, music_dir_str, None).await?;

            let track_fetcher = LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = implementations::LibrespotImageDownloader { session };

            process_track_cache(&track_fetcher, &audio_downloader, &image_downloader, &*track_provider, spotify_uri, &output_path, &file_id).await?;
            
            if !no_play {
                info!("\nStarting playback...");
                playback::play_audio_file(&output_path)?;
            }
        }
        SpotifyUri::Album { .. } => {
            let album_fetcher = implementations::LibrespotAlbumFetcher { session };
            let track_fetcher = implementations::LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = implementations::LibrespotImageDownloader { session };
            let cached_paths = cache_album(&album_fetcher, &track_fetcher, &audio_downloader, &image_downloader, spotify_uri, config).await?;
            
            if !no_play && !cached_paths.is_empty() {
                info!("\nStarting album playback...");
                playback::play_audio_files(&cached_paths)?;
            }
        }
        SpotifyUri::Playlist { .. } => {
            let playlist_fetcher = implementations::LibrespotPlaylistFetcher { session };
            let track_fetcher = implementations::LibrespotTrackFetcher { session };
            let audio_downloader = crate::stream::LibrespotAudioDownloader { session };
            let image_downloader = implementations::LibrespotImageDownloader { session };
            let cached_paths = cache_playlist(&playlist_fetcher, &track_fetcher, &audio_downloader, &image_downloader, spotify_uri, config).await?;
            
            if !no_play && !cached_paths.is_empty() {
                info!("\nStarting playlist playback...");
                playback::play_audio_files(&cached_paths)?;
            }
        }
        _ => {
            anyhow::bail!(
                "Unsupported URI type. Only track, album, and playlist URIs are supported."
            );
        }
    }

    Ok(())
}

/// Process multiple Spotify URIs with error handling and summary
async fn process_uris(
    session: &librespot_core::session::Session,
    uris: &[String],
    config: &Config,
    no_play: bool,
) -> anyhow::Result<()> {
    let mut successful = 0;
    let mut failed: Vec<(String, String)> = Vec::new();
    
    let show_progress = uris.len() > 1;
    
    for (index, uri_arg) in uris.iter().enumerate() {
        if show_progress {
            let current = index + 1;
            let total = uris.len();
            info!("Processing {} of {}: {}", current, total, uri_arg);
        }
        
        let spotify_uri = match parse_spotify_uri(uri_arg) {
            Ok(uri) => uri,
            Err(e) => {
                error!("❌ Failed to parse URI: {}", e);
                failed.push((uri_arg.clone(), e.to_string()));
                continue;
            }
        };
        
        match process_single_uri(session, &spotify_uri, config, no_play).await {
            Ok(_) => successful += 1,
            Err(e) => {
                error!("❌ Failed to process: {}", e);
                failed.push((uri_arg.clone(), e.to_string()));
            }
        }
    }
    
    // Show summary for multiple URIs or if there were any failures
    if uris.len() > 1 || !failed.is_empty() {
        info!("");
        info!("Summary:");
        info!("  Total: {}", uris.len());
        info!("  ✅ Successful: {}", successful);
        info!("  ❌ Failed: {}", failed.len());
        
        if !failed.is_empty() {
            info!("");
            info!("Failed URIs:");
            for (uri, error) in &failed {
                info!("  - {} ({})", uri, error);
            }
        }
    }
    
    // Return error only if ALL failed
    if successful == 0 && !failed.is_empty() {
        anyhow::bail!("All URIs failed to process");
    }
    
    Ok(())
}

/// Parse a Spotify URI from user input, handling various formats
pub fn parse_spotify_uri(uri_arg: &str) -> anyhow::Result<SpotifyUri> {
    if uri_arg.starts_with("spotify:") {
        SpotifyUri::from_uri(uri_arg)
            .map_err(|_| anyhow::anyhow!("Invalid Spotify URI: {}", uri_arg))
    } else {
        // Assume it's a track ID if no prefix
        let uri_string = format!("spotify:track:{}", uri_arg);
        SpotifyUri::from_uri(&uri_string)
            .map_err(|_| anyhow::anyhow!("Invalid track ID: {}", uri_arg))
    }
}

/// Validate command line arguments
pub fn validate_args(args: &[String]) -> anyhow::Result<(InputSource, bool)> {
    let mut input_source = None;
    let mut no_play = false;
    let mut iter = args.iter().skip(1).peekable();

    while let Some(arg) = iter.next() {
        if arg == "--no-play" {
            no_play = true;
        } else if arg == "--file" {
            if input_source.is_some() {
                anyhow::bail!("Cannot specify both --file and a URI");
            }
            let path = iter.next()
                .ok_or_else(|| anyhow::anyhow!("--file requires a path argument"))?;
            input_source = Some(InputSource::File(std::path::PathBuf::from(path)));
        } else if input_source.is_none() {
            input_source = Some(InputSource::SingleUri(arg.clone()));
        } else {
            anyhow::bail!("Unexpected argument: {}", arg);
        }
    }

    match input_source {
        Some(source) => Ok((source, no_play)),
        None => anyhow::bail!("Expected either a Spotify URI or --file <path>"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber with INFO level by default
    // Can be overridden with RUST_LOG environment variable
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Load configuration from environment variables
    let config = Config::from_env();

    let args: Vec<String> = env::args().collect();

    let (input_source, no_play) = match validate_args(&args) {
        Ok(result) => result,
        Err(_) => {
            eprintln!("Usage: {} [--no-play] <spotify_uri>", args[0]);
            eprintln!("       {} [--no-play] --file <path>", args[0]);
            eprintln!("Options:");
            eprintln!("  --no-play    Cache tracks without playing them");
            eprintln!("  --file       Read URIs from file (one per line, # for comments)");
            eprintln!();
            eprintln!("Examples:");
            eprintln!("  {} spotify:track:4uLU6hMCjMI75M1A2tKUQC", args[0]);
            eprintln!("  {} spotify:album:1A2GTWGtFfWp7KSQTwWOyo", args[0]);
            eprintln!("  {} --no-play spotify:playlist:37i9dQZF1DX0XUsuxWHRQd", args[0]);
            eprintln!("  {} 4uLU6hMCjMI75M1A2tKUQC  (assumes track)", args[0]);
            eprintln!("  {} --file my_uris.txt", args[0]);
            std::process::exit(1);
        }
    };

    let token_path = ".spotify_access_token";
    
    // Create session with automatic background token refresh
    let (session, _refresher, _refresh_handle) = loop {
        match create_session_with_auto_refresh(token_path).await {
            Ok(result) => break result,
            Err(e) if e.to_string().contains("Bad credentials") => {
                std::fs::remove_file(token_path).context("Failed to remove invalid token file")?;
                // Retry will trigger new OAuth flow
            }
            Err(e) => return Err(e),
        }
    };

    match input_source {
        InputSource::SingleUri(uri_arg) => {
            let uris = vec![uri_arg];
            process_uris(&session, &uris, &config, no_play).await?;
        }
        InputSource::File(path) => {
            let uris = read_uris_from_file(&path)?;
            info!("Loaded {} URIs from file", uris.len());
            process_uris(&session, &uris, &config, no_play).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_spotify_uri_with_track_prefix() {
        let uri = parse_spotify_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").unwrap();
        match uri {
            SpotifyUri::Track { .. } => {} // Success
            _ => panic!("Expected Track URI"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_with_album_prefix() {
        let uri = parse_spotify_uri("spotify:album:1A2GTWGtFfWp7KSQTwWOyo").unwrap();
        match uri {
            SpotifyUri::Album { .. } => {} // Success
            _ => panic!("Expected Album URI"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_with_playlist_prefix() {
        let uri = parse_spotify_uri("spotify:playlist:37i9dQZF1DX0XUsuxWHRQd").unwrap();
        match uri {
            SpotifyUri::Playlist { .. } => {} // Success
            _ => panic!("Expected Playlist URI"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_bare_id_assumes_track() {
        let uri = parse_spotify_uri("4uLU6hMCjMI75M1A2tKUQC").unwrap();
        match uri {
            SpotifyUri::Track { .. } => {} // Success
            _ => panic!("Expected Track URI for bare ID"),
        }
    }

    #[test]
    fn test_parse_spotify_uri_invalid() {
        let result = parse_spotify_uri("invalid:stuff:here");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_get_music_dir() {
        let config = Config::default();
        let music_dir = config.get_music_dir().expect("Failed to get music dir");
        assert!(music_dir.to_string_lossy().ends_with("Music"));
    }

    #[test]
    fn test_validate_args_correct_count() {
        let args = vec!["program".to_string(), "spotify:track:123".to_string()];
        let result = validate_args(&args);
        assert!(result.is_ok());
        let (input_source, no_play) = result.unwrap();
        match input_source {
            InputSource::SingleUri(uri) => assert_eq!(uri, "spotify:track:123"),
            _ => panic!("Expected SingleUri"),
        }
        assert!(!no_play);
    }

    #[test]
    fn test_validate_args_with_no_play() {
        let args = vec!["program".to_string(), "--no-play".to_string(), "spotify:track:123".to_string()];
        let result = validate_args(&args);
        assert!(result.is_ok());
        let (input_source, no_play) = result.unwrap();
        match input_source {
            InputSource::SingleUri(uri) => assert_eq!(uri, "spotify:track:123"),
            _ => panic!("Expected SingleUri"),
        }
        assert!(no_play);
    }

    #[test]
    fn test_validate_args_too_few() {
        let args = vec!["program".to_string()];
        let result = validate_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_unexpected_arg() {
        let args = vec![
            "program".to_string(),
            "spotify:track:123".to_string(),
            "extra".to_string(),
        ];
        let result = validate_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_with_file() {
        let args = vec!["program".to_string(), "--file".to_string(), "uris.txt".to_string()];
        let result = validate_args(&args);
        assert!(result.is_ok());
        let (input_source, no_play) = result.unwrap();
        match input_source {
            InputSource::File(path) => assert_eq!(path.to_str().unwrap(), "uris.txt"),
            _ => panic!("Expected File"),
        }
        assert!(!no_play);
    }

    #[test]
    fn test_validate_args_with_file_and_no_play() {
        let args = vec![
            "program".to_string(),
            "--no-play".to_string(),
            "--file".to_string(),
            "uris.txt".to_string(),
        ];
        let result = validate_args(&args);
        assert!(result.is_ok());
        let (input_source, no_play) = result.unwrap();
        match input_source {
            InputSource::File(path) => assert_eq!(path.to_str().unwrap(), "uris.txt"),
            _ => panic!("Expected File"),
        }
        assert!(no_play);
    }

    #[test]
    fn test_validate_args_file_without_path() {
        let args = vec!["program".to_string(), "--file".to_string()];
        let result = validate_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_both_file_and_uri() {
        let args = vec![
            "program".to_string(),
            "--file".to_string(),
            "uris.txt".to_string(),
            "spotify:track:123".to_string(),
        ];
        let result = validate_args(&args);
        assert!(result.is_err());
    }
}
