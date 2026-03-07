package playlist

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func entries(dir string) []TrackEntry {
	return []TrackEntry{
		{Path: filepath.Join(dir, "Artist", "Album", "01-Track One.ogg"), DurationSec: 210, Artist: "Artist", Title: "Track One"},
		{Path: filepath.Join(dir, "Artist", "Album", "02-Track Two.ogg"), DurationSec: 185, Artist: "Artist", Title: "Track Two"},
	}
}

func TestWriteM3U8BasicStructure(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "Artist", "Album", "album.m3u8")
	if err := os.MkdirAll(filepath.Join(dir, "Artist", "Album"), 0755); err != nil {
		t.Fatal(err)
	}

	err := WriteM3U8(dest, entries(dir), nil)
	if err != nil {
		t.Fatalf("WriteM3U8 error: %v", err)
	}

	data, err := os.ReadFile(dest)
	if err != nil {
		t.Fatalf("read playlist: %v", err)
	}
	content := string(data)

	if !strings.HasPrefix(content, "#EXTM3U\n") {
		t.Errorf("playlist must start with #EXTM3U, got: %q", content[:min(len(content), 20)])
	}
	if !strings.Contains(content, "#EXTINF:210,Artist - Track One") {
		t.Errorf("missing EXTINF for Track One:\n%s", content)
	}
	if !strings.Contains(content, "#EXTINF:185,Artist - Track Two") {
		t.Errorf("missing EXTINF for Track Two:\n%s", content)
	}
}

func TestWriteM3U8RelativePaths(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "Artist", "Album", "album.m3u8")
	if err := os.MkdirAll(filepath.Join(dir, "Artist", "Album"), 0755); err != nil {
		t.Fatal(err)
	}

	err := WriteM3U8(dest, entries(dir), nil)
	if err != nil {
		t.Fatal(err)
	}

	data, _ := os.ReadFile(dest)
	content := string(data)

	// Paths in the file must be relative (not absolute)
	if strings.Contains(content, dir) {
		t.Errorf("playlist contains absolute paths:\n%s", content)
	}
	if !strings.Contains(content, "01-Track One.ogg") {
		t.Errorf("expected relative filename in playlist:\n%s", content)
	}
}

func TestWriteM3U8Metadata(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "album.m3u8")
	meta := Metadata{
		"upc":         "602577904004",
		"spotify_uri": "spotify:album:abc",
		"empty_key":   "", // should be omitted
	}

	err := WriteM3U8(dest, entries(dir), meta)
	if err != nil {
		t.Fatal(err)
	}

	data, _ := os.ReadFile(dest)
	content := string(data)

	if !strings.Contains(content, "# upc=602577904004") {
		t.Errorf("missing UPC metadata:\n%s", content)
	}
	if !strings.Contains(content, "# spotify_uri=spotify:album:abc") {
		t.Errorf("missing spotify_uri metadata:\n%s", content)
	}
	if strings.Contains(content, "empty_key") {
		t.Errorf("empty metadata key should be omitted:\n%s", content)
	}
}

func TestWriteM3U8UnknownDuration(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "single.m3u8")
	es := []TrackEntry{
		{Path: filepath.Join(dir, "track.ogg"), DurationSec: 0, Title: "Unknown"},
	}

	if err := WriteM3U8(dest, es, nil); err != nil {
		t.Fatal(err)
	}

	data, _ := os.ReadFile(dest)
	// Duration 0 should be written as -1 (standard M3U8 for unknown)
	if !strings.Contains(string(data), "#EXTINF:-1,") {
		t.Errorf("expected -1 duration for unknown, got:\n%s", string(data))
	}
}

func TestWriteM3U8NoArtist(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "ep.m3u8")
	es := []TrackEntry{
		{Path: filepath.Join(dir, "ep.ogg"), DurationSec: 3600, Title: "Episode Title"},
	}

	if err := WriteM3U8(dest, es, nil); err != nil {
		t.Fatal(err)
	}

	data, _ := os.ReadFile(dest)
	// Without artist, EXTINF should show title only (no " - " prefix)
	if !strings.Contains(string(data), "#EXTINF:3600,Episode Title") {
		t.Errorf("unexpected EXTINF for no-artist entry:\n%s", string(data))
	}
}

func TestWriteM3U8CreatesDirectories(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "deep", "nested", "dir", "playlist.m3u8")

	if err := WriteM3U8(dest, nil, nil); err != nil {
		t.Fatalf("WriteM3U8 should create directories: %v", err)
	}
	if _, err := os.Stat(dest); err != nil {
		t.Errorf("playlist file not created: %v", err)
	}
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
