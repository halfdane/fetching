// Package tagger writes audio metadata tags and cover art into downloaded
// files using ffmpeg. It supports all formats that ffmpeg can handle
// (OGG Vorbis, AAC/M4A, MP3, etc.).
package tagger

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/halfdane/fetching/internal/spotify"
)

// Tagger writes metadata into audio files using ffmpeg.
type Tagger struct {
	// Binary is the ffmpeg executable name or path.
	Binary string
	// Verbose controls ffmpeg output. When false, ffmpeg output is suppressed.
	Verbose bool
	// cmdFunc creates the ffmpeg subprocess; defaults to exec.Command.
	// Override in tests to inject a fake binary without touching PATH.
	cmdFunc func(name string, args ...string) *exec.Cmd
}

// New creates a Tagger. If binary is empty, "ffmpeg" is used.
func New(binary string, verbose bool) *Tagger {
	if binary == "" {
		binary = "ffmpeg"
	}
	return &Tagger{Binary: binary, Verbose: verbose}
}

// TagTrack writes metadata and cover art into the audio file at audioPath.
// It uses all available information from the track metadata.
func (t *Tagger) TagTrack(audioPath string, track *spotify.Track) error {
	// Build metadata key-value pairs
	meta := map[string]string{
		"title":        track.Name,
		"album":        track.Album.Name,
		"track":        fmt.Sprintf("%d", track.Number),
		"disc":         fmt.Sprintf("%d", track.DiscNumber),
	}

	// Artists
	if len(track.Artists) > 0 {
		var names []string
		for _, a := range track.Artists {
			names = append(names, a.Name)
		}
		meta["artist"] = strings.Join(names, "; ")
	}

	// Album artist (from album metadata)
	if len(track.Album.Artists) > 0 {
		var names []string
		for _, a := range track.Album.Artists {
			names = append(names, a.Name)
		}
		meta["album_artist"] = strings.Join(names, "; ")
	}

	// Date from album
	if track.Album.Date != "" {
		meta["date"] = extractYear(track.Album.Date)
	}

	// Label
	if track.Album.Label != "" {
		meta["publisher"] = track.Album.Label
	}

	// ISRC
	if isrc := spotify.ISRC(track.ExternalIDs); isrc != "" {
		meta["isrc"] = isrc
	}

	// UPC from album
	if upc := spotify.UPC(track.Album.ExternalIDs); upc != "" {
		meta["upc"] = upc
	}

	// Spotify identifiers
	if track.URI != "" {
		meta["spotify_track_uri"] = track.URI
	}
	if track.Album.URI != "" {
		meta["spotify_album_uri"] = track.Album.URI
	}

	// Language
	if len(track.LanguageOfPerformance) > 0 {
		meta["language"] = track.LanguageOfPerformance[0]
	}

	// Cover art: prefer album covers from the track's album reference
	cover := spotify.DefaultCover(track.Album.Covers)

	return t.tag(audioPath, meta, cover)
}

// TagEpisode writes metadata and cover art into a podcast episode audio file.
func (t *Tagger) TagEpisode(audioPath string, ep *spotify.Episode) error {
	meta := map[string]string{
		"title":   ep.Name,
		"album":   ep.ShowName,
		"artist":  ep.ShowName,
		"track":   fmt.Sprintf("%d", ep.Number),
		"comment": ep.Description,
	}

	if ep.PublishTime != "" {
		meta["date"] = extractYear(ep.PublishTime)
	}

	if ep.Language != "" {
		meta["language"] = ep.Language
	}

	// Spotify identifiers
	if ep.URI != "" {
		meta["spotify_episode_uri"] = ep.URI
	}
	if ep.ShowURI != "" {
		meta["spotify_show_uri"] = ep.ShowURI
	}

	cover := spotify.DefaultCover(ep.Covers)

	return t.tag(audioPath, meta, cover)
}

// tag runs ffmpeg to write metadata and optionally embed cover art.
func (t *Tagger) tag(audioPath string, meta map[string]string, cover *spotify.Cover) error {
	// Download cover to a temp file if available
	var coverPath string
	if cover != nil {
		var err error
		coverPath, err = downloadCover(cover.FileID)
		if err != nil {
			log.Printf("warning: failed to download cover art: %v", err)
			// Continue without cover
		} else {
			defer os.Remove(coverPath)
		}
	}

	// Build ffmpeg command
	// Strategy: copy the audio stream, add metadata, output to temp file, then rename.
	// Preserve the original extension so ffmpeg can detect the output format.
	origExt := filepath.Ext(audioPath)
	tmpOut := strings.TrimSuffix(audioPath, origExt) + ".tagged" + origExt
	args := t.buildArgs(audioPath, tmpOut, meta, coverPath)

	cmd := exec.Command
	if t.cmdFunc != nil {
		cmd = t.cmdFunc
	}
	ffmpeg := cmd(t.Binary, args...)
	if t.Verbose {
		ffmpeg.Stderr = os.Stderr
	}

	if err := ffmpeg.Run(); err != nil {
		os.Remove(tmpOut)
		return fmt.Errorf("ffmpeg tag %q: %w", audioPath, err)
	}

	// Replace original with tagged version
	if err := os.Rename(tmpOut, audioPath); err != nil {
		os.Remove(tmpOut)
		return fmt.Errorf("replace with tagged file: %w", err)
	}

	return nil
}

func (t *Tagger) buildArgs(input, output string, meta map[string]string, coverPath string) []string {
	ext := strings.ToLower(filepath.Ext(input))
	isOGG := ext == ".ogg"

	args := []string{}

	// Suppress ffmpeg output unless verbose.
	if !t.Verbose {
		args = append(args, "-hide_banner", "-loglevel", "error")
	}

	args = append(args,
		"-y",        // overwrite output
		"-i", input, // input audio
	)

	// Add cover image as second input for formats that support video streams.
	// OGG uses METADATA_BLOCK_PICTURE instead (handled below).
	if coverPath != "" && !isOGG {
		args = append(args, "-i", coverPath)
	}

	// Copy audio stream without re-encoding
	args = append(args, "-c:a", "copy")

	// Handle cover art embedding based on format
	if coverPath != "" {
		if isOGG {
			// OGG Vorbis: encode cover as METADATA_BLOCK_PICTURE Vorbis comment.
			picB64, err := buildMetadataBlockPicture(coverPath)
			if err != nil {
				log.Printf("warning: could not encode cover for OGG: %v", err)
			} else {
				args = append(args, "-metadata", "METADATA_BLOCK_PICTURE="+picB64)
			}
		} else {
			// MP3, M4A, FLAC, etc.: embed cover as attached picture via video stream.
			args = append(args,
				"-map", "0:a",
				"-map", "1:v",
				"-c:v", "copy",
				"-disposition:v", "attached_pic",
			)
			if ext == ".mp3" {
				args = append(args, "-id3v2_version", "3")
			}
		}
	}

	// Add metadata tags
	for key, value := range meta {
		if value != "" {
			args = append(args, "-metadata", key+"="+value)
		}
	}

	args = append(args, output)
	return args
}

// buildMetadataBlockPicture encodes a cover image file as a base64
// FLAC Picture block suitable for the METADATA_BLOCK_PICTURE Vorbis comment.
func buildMetadataBlockPicture(coverPath string) (string, error) {
	data, err := os.ReadFile(coverPath)
	if err != nil {
		return "", fmt.Errorf("read cover file: %w", err)
	}

	mime := "image/jpeg"
	if strings.HasSuffix(strings.ToLower(coverPath), ".png") {
		mime = "image/png"
	}

	var buf bytes.Buffer
	// Picture type: 3 = front cover
	_ = binary.Write(&buf, binary.BigEndian, uint32(3))
	// MIME type
	_ = binary.Write(&buf, binary.BigEndian, uint32(len(mime)))
	buf.WriteString(mime)
	// Description (empty)
	_ = binary.Write(&buf, binary.BigEndian, uint32(0))
	// Width
	_ = binary.Write(&buf, binary.BigEndian, uint32(300))
	// Height
	_ = binary.Write(&buf, binary.BigEndian, uint32(300))
	// Color depth (24-bit for JPEG)
	_ = binary.Write(&buf, binary.BigEndian, uint32(24))
	// Number of indexed colors (0 for non-indexed)
	_ = binary.Write(&buf, binary.BigEndian, uint32(0))
	// Image data length + data
	_ = binary.Write(&buf, binary.BigEndian, uint32(len(data)))
	buf.Write(data)

	return base64.StdEncoding.EncodeToString(buf.Bytes()), nil
}

// downloadCover fetches a cover image from Spotify's CDN and saves it to a temp file.
func downloadCover(fileID string) (string, error) {
	url := spotify.CoverURL(fileID)

	resp, err := http.Get(url)
	if err != nil {
		return "", fmt.Errorf("download cover from %s: %w", url, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("cover download returned status %d", resp.StatusCode)
	}

	// Determine extension from content type
	ext := ".jpg"
	if ct := resp.Header.Get("Content-Type"); strings.Contains(ct, "png") {
		ext = ".png"
	}

	tmpFile, err := os.CreateTemp("", "fetching-cover-*"+ext)
	if err != nil {
		return "", fmt.Errorf("create temp cover file: %w", err)
	}
	defer tmpFile.Close()

	if _, err := io.Copy(tmpFile, resp.Body); err != nil {
		os.Remove(tmpFile.Name())
		return "", fmt.Errorf("write cover data: %w", err)
	}

	return tmpFile.Name(), nil
}

// extractYear pulls the year from a date string like "1994-09-13 0:00:00.0 +00:00:00".
func extractYear(dateStr string) string {
	if len(dateStr) >= 4 {
		return dateStr[:4]
	}
	return dateStr
}
