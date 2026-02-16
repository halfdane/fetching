# fetching

A lightweight Spotify streaming player written in Rust. Streams tracks, albums, and playlists with local caching for offline playback.

## Features

- 🎵 Stream tracks, albums, and playlists
- 💾 Local caching for offline playback (OGG Vorbis, up to 320kbps)
- 🏷️ Full metadata tagging (artist, album, year, genre, ISRC)
- 🖼️ Album art embedding
- 🔐 OAuth authentication with browser login
- 📝 M3U8 playlist generation

## Requirements

- Rust 1.70+
- Spotify Premium account

## Build

```bash
cargo build --release
```

## Usage

```bash
# Stream a track
fetching spotify:track:4uLU6hMCjMI75M1A2tKUQC

# Stream an album
fetching spotify:album:1A2GTWGtFfWp7KSQTwWOyo

# Stream a playlist
fetching spotify:playlist:37i9dQZF1DX0XUsuxWHRQd

# Cache without playback
fetching --no-play spotify:album:1A2GTWGtFfWp7KSQTwWOyo
```

Cached files are stored in `~/Music/` organized by artist and album.

### First Run

On first run, the application will:
1. Open your browser for Spotify OAuth authentication
2. Save the access token to `.spotify_access_token`
3. Cache credentials in `.cache/` directory

Subsequent runs will reuse the cached credentials automatically.
## Configuration

Environment variables (all optional):

| Variable | Default | Description |
|----------|---------|-------------|
| `MUSIC_DIR` | `~/Music` | Cache directory |
| `TRACK_DELAY_MS` | `200` | Delay between operations (ms) |
| `RUST_LOG` | `info` | Logging level |

## License

[Specify your license here]

## Acknowledgments

Built with [librespot](https://github.com/librespot-org/librespot).

---

**Note**: For personal use only. Respects Spotify's streaming infrastructure similar to official mobile apps with offline mode.
