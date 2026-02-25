use async_trait::async_trait;
use librespot_core::SpotifyUri;

use crate::container::{CollectionType, Track, TrackCollection, to_collection_type};

/// Convert an `https://open.spotify.com/{type}/{id}` URL to a
/// `spotify:{type}:{id}` URI, stripping any query string / fragment first.
///
/// Inputs that are already `spotify:` URIs are returned unchanged.
pub fn normalise_uri(input: &str) -> anyhow::Result<String> {
    const PREFIX: &str = "https://open.spotify.com/";

    if input.starts_with("spotify:") {
        return Ok(input.to_owned());
    }

    if let Some(rest) = input.strip_prefix(PREFIX) {
        // Drop query string ("?si=...") or fragment ("#...")
        let path = rest.split(&['?', '#']).next().unwrap_or(rest);

        // Expect exactly two path segments: "{type}/{id}"
        let mut parts = path.splitn(2, '/');
        let item_type = parts.next().unwrap_or("").trim();
        let item_id   = parts.next().unwrap_or("").trim();

        anyhow::ensure!(
            !item_type.is_empty() && !item_id.is_empty(),
            "Cannot parse Spotify URL — expected https://open.spotify.com/{{type}}/{{id}}, got: {input}"
        );

        return Ok(format!("spotify:{item_type}:{item_id}"));
    }

    anyhow::bail!(
        "Unrecognised Spotify identifier '{input}' — \
         expected a spotify:… URI or https://open.spotify.com/… URL"
    )
}

pub trait SpotifyCollectionMetadata {
    fn fetch_album(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_playlist(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_track(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_episode(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;
    fn fetch_show(&self, uri: &SpotifyUri) -> anyhow::Result<TrackCollection>;

    fn fetch_by_uri(&self, uri_str: &str) -> anyhow::Result<TrackCollection> {
        let normalised = normalise_uri(uri_str)?;
        let spotify_uri = &SpotifyUri::from_uri(&normalised)?;

        let collection = match to_collection_type(spotify_uri)? {
            CollectionType::Album => self.fetch_album(spotify_uri)?,
            CollectionType::SingleTrack => self.fetch_track(spotify_uri)?,
            CollectionType::Playlist => self.fetch_playlist(spotify_uri)?,
            CollectionType::Show => self.fetch_show(spotify_uri)?,
            CollectionType::SingleEpisode => self.fetch_episode(spotify_uri)?,
        };

        Ok(collection)
    }
}

pub trait SpotifyTrackMetadata {
    fn fetch_single_episode(
        &self,
        spotify_uri: &SpotifyUri,
    ) -> anyhow::Result<Track>;
    fn fetch_single_track(
        &self,
        spotify_uri: &SpotifyUri,
    ) -> anyhow::Result<Track>;

    fn fetch_by_uri(&self, uri_str: &str) -> anyhow::Result<Track> {
        let normalised = normalise_uri(uri_str)?;
        let spotify_uri = &SpotifyUri::from_uri(&normalised)?;

        match spotify_uri.item_type() {
            "track" => self.fetch_single_track(spotify_uri),
            "episode" => self.fetch_single_episode(spotify_uri),
            _ => anyhow::bail!("Unsupported URI type: {}", uri_str),
        }
    }
}

#[async_trait]
pub trait SpotifyCover: Clone + Send + Sync {
    async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>>;
}

#[cfg(test)]
mod tests {
    use super::normalise_uri;

    #[test]
    fn spotify_uri_passes_through_unchanged() {
        assert_eq!(
            normalise_uri("spotify:album:2moJkIdXsBQBpBVJiu2IVR").unwrap(),
            "spotify:album:2moJkIdXsBQBpBVJiu2IVR"
        );
    }

    #[test]
    fn https_url_album_is_converted() {
        assert_eq!(
            normalise_uri("https://open.spotify.com/album/2moJkIdXsBQBpBVJiu2IVR").unwrap(),
            "spotify:album:2moJkIdXsBQBpBVJiu2IVR"
        );
    }

    #[test]
    fn https_url_track_is_converted() {
        assert_eq!(
            normalise_uri("https://open.spotify.com/track/4iV5W9uYEdYUVa79Axb7Rh").unwrap(),
            "spotify:track:4iV5W9uYEdYUVa79Axb7Rh"
        );
    }

    #[test]
    fn https_url_with_si_query_param_is_stripped() {
        assert_eq!(
            normalise_uri("https://open.spotify.com/album/2moJkIdXsBQBpBVJiu2IVR?si=abc123").unwrap(),
            "spotify:album:2moJkIdXsBQBpBVJiu2IVR"
        );
    }

    #[test]
    fn https_url_playlist_is_converted() {
        assert_eq!(
            normalise_uri("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=xyz").unwrap(),
            "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"
        );
    }

    #[test]
    fn https_url_show_is_converted() {
        assert_eq!(
            normalise_uri("https://open.spotify.com/show/4rOoJ6Egrf8K2IrywzwOMk").unwrap(),
            "spotify:show:4rOoJ6Egrf8K2IrywzwOMk"
        );
    }

    #[test]
    fn https_url_episode_is_converted() {
        assert_eq!(
            normalise_uri("https://open.spotify.com/episode/7makk4oTQel546B0PZlDM5").unwrap(),
            "spotify:episode:7makk4oTQel546B0PZlDM5"
        );
    }

    #[test]
    fn garbage_input_returns_error() {
        assert!(normalise_uri("not-a-spotify-thing").is_err());
    }

    #[test]
    fn https_url_missing_id_returns_error() {
        assert!(normalise_uri("https://open.spotify.com/album/").is_err());
    }
}
