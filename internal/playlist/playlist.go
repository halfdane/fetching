// Package playlist generates M3U8 playlist files and composite cover images
// for albums, shows, and playlists.
package playlist

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// TrackEntry holds the information needed for one line in an M3U8 file.
type TrackEntry struct {
	// Path is the absolute path to the audio file on disk.
	Path string
	// DurationSec is the track duration in seconds (-1 if unknown).
	DurationSec int
	// Artist is the display artist name.
	Artist string
	// Title is the track/episode title.
	Title string
}

// Metadata holds optional key-value pairs written as #EXTGRP / x-attributes
// in the M3U8 header comment block.
type Metadata map[string]string

// WriteM3U8 writes an Extended M3U playlist file at dest.
// Paths in the file are relative to the directory containing dest.
func WriteM3U8(dest string, entries []TrackEntry, meta Metadata) error {
	dir := filepath.Dir(dest)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("create playlist directory: %w", err)
	}

	var b strings.Builder
	b.WriteString("#EXTM3U\n")

	// Write metadata as comments.
	for k, v := range meta {
		if v != "" {
			b.WriteString(fmt.Sprintf("# %s=%s\n", k, v))
		}
	}

	for _, e := range entries {
		rel, err := filepath.Rel(dir, e.Path)
		if err != nil {
			rel = e.Path // fallback to absolute
		}
		dur := e.DurationSec
		if dur <= 0 {
			dur = -1
		}
		display := e.Title
		if e.Artist != "" {
			display = e.Artist + " - " + e.Title
		}
		b.WriteString(fmt.Sprintf("#EXTINF:%d,%s\n", dur, display))
		b.WriteString(rel + "\n")
	}

	return os.WriteFile(dest, []byte(b.String()), 0644)
}
