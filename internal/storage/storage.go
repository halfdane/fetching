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

// CreateTrackWriter returns a WriteCloser for storing a track's audio file.
// The file path follows: <base>/<artist>/<album>/<number>-<track>.ogg
func (s *Storage) CreateTrackWriter(track *spotify.Track) (io.WriteCloser, error) {
	artist := sanitize(firstArtist(track.Artists))
	album := sanitize(track.Album.Name)
	filename := fmt.Sprintf("%02d-%s.ogg", track.Number, sanitize(track.Name))

	dir := filepath.Join(s.BaseDir, artist, album)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return nil, fmt.Errorf("create directory %s: %w", dir, err)
	}

	path := filepath.Join(dir, filename)
	f, err := os.Create(path)
	if err != nil {
		return nil, fmt.Errorf("create file %s: %w", path, err)
	}
	return f, nil
}

// CreateEpisodeWriter returns a WriteCloser for storing an episode's audio file.
// The file path follows: <base>/<show>/<episode>.ogg
func (s *Storage) CreateEpisodeWriter(ep *spotify.Episode) (io.WriteCloser, error) {
	show := sanitize(ep.ShowURI) // Best-effort; could be improved with show metadata
	filename := sanitize(ep.Name) + ".ogg"

	dir := filepath.Join(s.BaseDir, show)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return nil, fmt.Errorf("create directory %s: %w", dir, err)
	}

	path := filepath.Join(dir, filename)
	f, err := os.Create(path)
	if err != nil {
		return nil, fmt.Errorf("create file %s: %w", path, err)
	}
	return f, nil
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
