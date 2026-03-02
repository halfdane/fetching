// Package storage handles writing downloaded audio files to disk using a
// structured directory layout: <base>/<artist>/<album>/<track>.ogg
package storage

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/halfdane/fetching/internal/spotify"
)

// Storage manages the output directory for downloaded files.
type Storage struct {
	BaseDir string
}

// New creates a Storage rooted at the given directory.
func New(baseDir string) *Storage {
	return &Storage{BaseDir: baseDir}
}

// CreateTrackWriter returns the output file path and a WriteCloser for storing
// a track's audio file.
// The file path follows: <base>/<artist>/<album>/<number>-<track><ext>
func (s *Storage) CreateTrackWriter(track *spotify.Track, ext string) (string, io.WriteCloser, error) {
	artist := sanitize(firstArtist(track.Artists))
	album := sanitize(track.Album.Name)
	filename := fmt.Sprintf("%02d-%s%s", track.Number, sanitize(track.Name), ext)

	dir := filepath.Join(s.BaseDir, artist, album)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return "", nil, fmt.Errorf("create directory %s: %w", dir, err)
	}

	path := filepath.Join(dir, filename)
	f, err := os.Create(path)
	if err != nil {
		return "", nil, fmt.Errorf("create file %s: %w", path, err)
	}
	return path, f, nil
}

// CreateEpisodeWriter returns the output file path and a WriteCloser for
// storing an episode's audio file.
// The file path follows: <base>/<show>/<episode><ext>
func (s *Storage) CreateEpisodeWriter(ep *spotify.Episode, ext string) (string, io.WriteCloser, error) {
	show := sanitize(ep.ShowName)
	if show == "_" {
		show = sanitize(ep.ShowURI) // fallback
	}
	filename := sanitize(ep.Name) + ext

	dir := filepath.Join(s.BaseDir, show)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return "", nil, fmt.Errorf("create directory %s: %w", dir, err)
	}

	path := filepath.Join(dir, filename)
	f, err := os.Create(path)
	if err != nil {
		return "", nil, fmt.Errorf("create file %s: %w", path, err)
	}
	return path, f, nil
}

func firstArtist(artists []spotify.Artist) string {
	if len(artists) == 0 {
		return "Unknown Artist"
	}
	return artists[0].Name
}

// sanitize replaces filesystem-unfriendly characters with underscores.
func sanitize(name string) string {
	if name == "" {
		return "_"
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
	s := replacer.Replace(name)
	// Trim leading/trailing dots and spaces
	s = strings.Trim(s, ". ")
	if s == "" {
		return "_"
	}
	return s
}

// Sanitize is exported for use by other packages that need filesystem-safe names.
func Sanitize(name string) string {
	return sanitize(name)
}

// AlbumDir returns the directory path for an album: <base>/<artist>/<album>.
func (s *Storage) AlbumDir(artist, album string) string {
	return filepath.Join(s.BaseDir, sanitize(artist), sanitize(album))
}

// ShowDir returns the directory path for a show: <base>/<show>.
func (s *Storage) ShowDir(showName string) string {
	return filepath.Join(s.BaseDir, sanitize(showName))
}

// PlaylistDir returns the directory path for a playlist: <base>/Playlists/<name>.
func (s *Storage) PlaylistDir(playlistName string) string {
	return filepath.Join(s.BaseDir, "Playlists", sanitize(playlistName))
}

// TrackPath returns the expected file path for a track (without creating it).
func (s *Storage) TrackPath(track *spotify.Track, ext string) string {
	artist := sanitize(firstArtist(track.Artists))
	album := sanitize(track.Album.Name)
	filename := fmt.Sprintf("%02d-%s%s", track.Number, sanitize(track.Name), ext)
	return filepath.Join(s.BaseDir, artist, album, filename)
}

// EpisodePath returns the expected file path for an episode (without creating it).
func (s *Storage) EpisodePath(ep *spotify.Episode, ext string) string {
	show := sanitize(ep.ShowName)
	if show == "_" {
		show = sanitize(ep.ShowURI)
	}
	return filepath.Join(s.BaseDir, show, sanitize(ep.Name)+ext)
}
