// Package pathtemplate resolves {token} path templates for audio file naming.
//
// Tokens are enclosed in curly braces, e.g. {artist}/{album}/{track_number}-{title}.
// The resolved path is relative to the storage base directory; the file extension
// is always appended automatically and must not be included in the template.
//
// All token values are sanitized to be filesystem-safe before substitution.
// Path separators in the template are preserved so templates can contain
// subdirectory structure.
//
// Default templates:
//
//	Track:   {artist}/{album}/{track_number}-{title}
//	Episode: {show}/{title}
package pathtemplate

import (
	"fmt"
	"strings"
)

// DefaultTrack is the default path template for music tracks.
const DefaultTrack = "{artist}/{album}/{track_number}-{title}"

// DefaultEpisode is the default path template for podcast episodes.
const DefaultEpisode = "{show}/{title}"

// TrackTokens holds the values used when expanding a track template.
type TrackTokens struct {
	Artist      string // first performing artist
	AlbumArtist string // first album artist (falls back to Artist)
	Album       string
	Title       string
	TrackNumber int
	DiscNumber  int
	Year        string // first 4 chars of album date, e.g. "2003"
}

// EpisodeTokens holds the values used when expanding an episode template.
type EpisodeTokens struct {
	Show          string
	Title         string
	Year          string // first 4 chars of publish_time
	EpisodeNumber int
}

// ExpandTrack expands tmpl using the given tokens.
// Returns an error if the template contains an unrecognised token.
func ExpandTrack(tmpl string, t TrackTokens) (string, error) {
	disc := ""
	if t.DiscNumber > 1 {
		disc = fmt.Sprintf("%d", t.DiscNumber)
	}
	tokens := map[string]string{
		"artist":       t.Artist,
		"album_artist": t.AlbumArtist,
		"album":        t.Album,
		"title":        t.Title,
		"track_number": fmt.Sprintf("%02d", t.TrackNumber),
		"disc_number":  disc,
		"year":         t.Year,
	}
	return expand(tmpl, tokens)
}

// ExpandEpisode expands tmpl using the given episode tokens.
func ExpandEpisode(tmpl string, e EpisodeTokens) (string, error) {
	tokens := map[string]string{
		"show":           e.Show,
		"title":          e.Title,
		"year":           e.Year,
		"episode_number": fmt.Sprintf("%02d", e.EpisodeNumber),
	}
	return expand(tmpl, tokens)
}

// expand performs {token} substitution. Each path segment of the template
// is processed independently. Token values are sanitized, including "/" so
// they cannot inject extra path segments.
func expand(tmpl string, tokens map[string]string) (string, error) {
	// Split the template on "/" to get per-segment pieces.
	segments := strings.Split(tmpl, "/")
	for i, seg := range segments {
		expanded, err := expandSegment(seg, tokens)
		if err != nil {
			return "", err
		}
		segments[i] = expanded
	}
	result := strings.Join(segments, "/")
	// Collapse any empty segments produced by empty optional tokens.
	for strings.Contains(result, "//") {
		result = strings.ReplaceAll(result, "//", "/")
	}
	result = strings.Trim(result, "/")
	if result == "" {
		result = "_"
	}
	return result, nil
}

// expandSegment substitutes tokens within a single path segment and sanitizes
// the result.
func expandSegment(seg string, tokens map[string]string) (string, error) {
	var b strings.Builder
	remaining := seg
	for {
		start := strings.Index(remaining, "{")
		if start == -1 {
			b.WriteString(sanitize(remaining))
			break
		}
		end := strings.Index(remaining[start:], "}")
		if end == -1 {
			return "", fmt.Errorf("unclosed '{' in template segment %q", seg)
		}
		end += start // absolute index

		// Text before the token — sanitize literal text
		b.WriteString(sanitize(remaining[:start]))

		token := remaining[start+1 : end]
		val, ok := tokens[token]
		if !ok {
			return "", fmt.Errorf("unknown template token {%s}", token)
		}
		b.WriteString(sanitize(val))
		remaining = remaining[end+1:]
	}
	s := b.String()
	// Trim leading/trailing dots and spaces from the whole segment.
	s = strings.Trim(s, ". ")
	if s == "" {
		return "_", nil
	}
	return s, nil
}

// sanitize replaces filesystem-unfriendly characters with underscores.
// This includes "/" so that token values cannot inject extra path segments.
func sanitize(s string) string {
	if s == "" {
		return ""
	}
	replacer := strings.NewReplacer(
		"/", "_",
		"\\", "_",
		":", "_",
		"*", "_",
		"?", "_",
		"\"", "_",
		"<", "_",
		">", "_",
		"|", "_",
		"\x00", "_",
	)
	return replacer.Replace(s)
}

// Validate returns an error if tmpl contains an unknown token, using zero-value
// tokens so all known tokens are recognised.
func ValidateTrack(tmpl string) error {
	_, err := ExpandTrack(tmpl, TrackTokens{})
	return err
}

// ValidateEpisode returns an error if tmpl contains an unknown token.
func ValidateEpisode(tmpl string) error {
	_, err := ExpandEpisode(tmpl, EpisodeTokens{})
	return err
}
