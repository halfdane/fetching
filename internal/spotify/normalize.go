package spotify

import (
	"net/url"
	"strings"
)

// NormalizeURI cleans a user-supplied Spotify URI or URL before it is stored
// in the queue or passed to fetching-cli.
//
// Spotify's "Share → Copy Link" produces URLs like:
//
//	https://open.spotify.com/album/7u20BJTgQ…?si=UXCM7hLZQ…
//
// The ?si= (and any other query parameters) are tracking tokens that
// fetching-cli does not understand and will cause the fetch to fail.
// They are stripped here so the stored URI is always clean.
//
// spotify: scheme URIs (e.g. "spotify:album:7u20BJTgQ…") are returned as-is.
func NormalizeURI(input string) string {
	input = strings.TrimSpace(input)
	if !strings.HasPrefix(input, "https://") && !strings.HasPrefix(input, "http://") {
		return input // already a spotify: URI or something else — pass through
	}
	u, err := url.Parse(input)
	if err != nil {
		return input // unparseable — pass through and let fetching-cli error
	}
	u.RawQuery = ""
	u.Fragment = ""
	return u.String()
}
