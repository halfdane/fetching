// Package storage handles writing downloaded audio files to disk using a
// path template: tokens like {artist}, {album}, {track_number} are expanded
// to form the path relative to the base directory.
package storage

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/halfdane/fetching/internal/pathtemplate"
	"github.com/halfdane/fetching/internal/spotify"
)

// Storage manages the output directory for downloaded files.
type Storage struct {
	BaseDir         string
	TrackTemplate   string
	EpisodeTemplate string
}

// New creates a Storage rooted at the given directory using the default templates.
func New(baseDir string) *Storage {
	return &Storage{
		BaseDir:         baseDir,
		TrackTemplate:   pathtemplate.DefaultTrack,
		EpisodeTemplate: pathtemplate.DefaultEpisode,
	}
}

// NewWithTemplates creates a Storage with custom path templates.
// Pass empty strings to use the defaults.
func NewWithTemplates(baseDir, trackTmpl, episodeTmpl string) *Storage {
	s := New(baseDir)
	if trackTmpl != "" {
		s.TrackTemplate = trackTmpl
	}
	if episodeTmpl != "" {
		s.EpisodeTemplate = episodeTmpl
	}
	return s
}

// withinBaseDir returns an error if path does not resolve to a location inside
// (or equal to) the base directory, preventing path traversal attacks.
func (s *Storage) withinBaseDir(path string) error {
	base := filepath.Clean(s.BaseDir)
	clean := filepath.Clean(path)
	if clean != base && !strings.HasPrefix(clean, base+string(os.PathSeparator)) {
		return fmt.Errorf("path %q escapes base directory %q", clean, base)
	}
	return nil
}

// CreateTrackWriter returns the output file path and a WriteCloser for storing
// a track's audio file.
func (s *Storage) CreateTrackWriter(track *spotify.Track, ext string) (string, io.WriteCloser, error) {
	path := s.TrackPath(track, ext)
	if err := s.withinBaseDir(path); err != nil {
		return "", nil, err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return "", nil, fmt.Errorf("create directory %s: %w", filepath.Dir(path), err)
	}
	f, err := os.Create(path)
	if err != nil {
		return "", nil, fmt.Errorf("create file %s: %w", path, err)
	}
	return path, f, nil
}

// CreateEpisodeWriter returns the output file path and a WriteCloser for
// storing an episode's audio file.
func (s *Storage) CreateEpisodeWriter(ep *spotify.Episode, ext string) (string, io.WriteCloser, error) {
	path := s.EpisodePath(ep, ext)
	if err := s.withinBaseDir(path); err != nil {
		return "", nil, err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return "", nil, fmt.Errorf("create directory %s: %w", filepath.Dir(path), err)
	}
	f, err := os.Create(path)
	if err != nil {
		return "", nil, fmt.Errorf("create file %s: %w", path, err)
	}
	return path, f, nil
}

// PlaylistDir returns the directory path for a playlist: <base>/Playlists/<name>.
// Playlists collect tracks from many albums so they get their own dedicated dir.
func (s *Storage) PlaylistDir(playlistName string) string {
	return filepath.Join(s.BaseDir, "Playlists", Sanitize(playlistName))
}

// TrackPath returns the expected file path for a track (without creating it).
func (s *Storage) TrackPath(track *spotify.Track, ext string) string {
	rel, err := pathtemplate.ExpandTrack(s.TrackTemplate, trackTokens(track))
	if err != nil {
		rel = fmt.Sprintf("_/%02d-%s", track.Number, Sanitize(track.Name))
	}
	return filepath.Join(s.BaseDir, filepath.FromSlash(rel)) + ext
}

// EpisodePath returns the expected file path for an episode (without creating it).
func (s *Storage) EpisodePath(ep *spotify.Episode, ext string) string {
	rel, err := pathtemplate.ExpandEpisode(s.EpisodeTemplate, episodeTokens(ep))
	if err != nil {
		rel = fmt.Sprintf("_/%s", Sanitize(ep.Name))
	}
	return filepath.Join(s.BaseDir, filepath.FromSlash(rel)) + ext
}

// Sanitize is exported for use by other packages that need filesystem-safe names.
func Sanitize(name string) string {
	return sanitize(name)
}

// sanitize replaces filesystem-unfriendly characters with underscores.
func sanitize(name string) string {
	if name == "" {
		return "_"
	}
	result := make([]byte, 0, len(name))
	for i := 0; i < len(name); i++ {
		switch name[i] {
		case '/', '\\', ':', '*', '?', '"', '<', '>', '|', 0:
			result = append(result, '_')
		default:
			result = append(result, name[i])
		}
	}
	s := string(result)
	for len(s) > 0 && (s[0] == '.' || s[0] == ' ') {
		s = s[1:]
	}
	for len(s) > 0 && (s[len(s)-1] == '.' || s[len(s)-1] == ' ') {
		s = s[:len(s)-1]
	}
	if s == "" {
		return "_"
	}
	return s
}

// trackTokens builds a TrackTokens struct from a Spotify track.
func trackTokens(track *spotify.Track) pathtemplate.TrackTokens {
	artist := "Unknown Artist"
	if len(track.Artists) > 0 {
		artist = track.Artists[0].Name
	}
	albumArtist := artist
	if len(track.Album.Artists) > 0 {
		albumArtist = track.Album.Artists[0].Name
	}
	year := ""
	if len(track.Album.Date) >= 4 {
		year = track.Album.Date[:4]
	}
	return pathtemplate.TrackTokens{
		Artist:      artist,
		AlbumArtist: albumArtist,
		Album:       track.Album.Name,
		Title:       track.Name,
		TrackNumber: track.Number,
		DiscNumber:  track.DiscNumber,
		Year:        year,
	}
}

// episodeTokens builds an EpisodeTokens struct from a Spotify episode.
func episodeTokens(ep *spotify.Episode) pathtemplate.EpisodeTokens {
	show := ep.ShowName
	if show == "" {
		show = ep.ShowURI
	}
	year := ""
	if len(ep.PublishTime) >= 4 {
		year = ep.PublishTime[:4]
	}
	return pathtemplate.EpisodeTokens{
		Show:          show,
		Title:         ep.Name,
		Year:          year,
		EpisodeNumber: ep.Number,
	}
}
