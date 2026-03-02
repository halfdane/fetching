// Package spotify provides parsing utilities for Spotify metadata JSON.
package spotify

import (
	"encoding/json"
	"fmt"
)

// typeProbe is used to peek at the "type" field in raw JSON.
type typeProbe struct {
	Type ItemType `json:"type"`
}

// ParseMetadata parses raw JSON from fetching-cli and returns the
// appropriate typed struct (Track, Album, Playlist, Show, or Episode).
func ParseMetadata(data []byte) (any, error) {
	var probe typeProbe
	if err := json.Unmarshal(data, &probe); err != nil {
		return nil, fmt.Errorf("parse metadata type: %w", err)
	}

	switch probe.Type {
	case TypeTrack:
		var t Track
		if err := json.Unmarshal(data, &t); err != nil {
			return nil, fmt.Errorf("parse track metadata: %w", err)
		}
		return &t, nil

	case TypeAlbum:
		var a Album
		if err := json.Unmarshal(data, &a); err != nil {
			return nil, fmt.Errorf("parse album metadata: %w", err)
		}
		return &a, nil

	case TypePlaylist:
		var p Playlist
		if err := json.Unmarshal(data, &p); err != nil {
			return nil, fmt.Errorf("parse playlist metadata: %w", err)
		}
		return &p, nil

	case TypeShow:
		var s Show
		if err := json.Unmarshal(data, &s); err != nil {
			return nil, fmt.Errorf("parse show metadata: %w", err)
		}
		return &s, nil

	case TypeEpisode:
		var e Episode
		if err := json.Unmarshal(data, &e); err != nil {
			return nil, fmt.Errorf("parse episode metadata: %w", err)
		}
		return &e, nil

	default:
		return nil, fmt.Errorf("unknown metadata type: %q", probe.Type)
	}
}

// TrackURIs extracts individual track/episode URIs from a metadata result.
// For a single track or episode it returns that item's URI.
// For albums, playlists, and shows it returns the contained track/episode URIs.
func TrackURIs(meta any) []string {
	switch v := meta.(type) {
	case *Track:
		return []string{v.URI}
	case *Episode:
		return []string{v.URI}
	case *Album:
		var uris []string
		for _, disc := range v.Discs {
			uris = append(uris, disc.Tracks...)
		}
		return uris
	case *Playlist:
		return v.TrackURIs
	case *Show:
		return v.EpisodeURIs
	default:
		return nil
	}
}

// DefaultCover returns the DEFAULT-sized cover from a covers slice,
// falling back to the first available cover. Returns nil if none exist.
func DefaultCover(covers []Cover) *Cover {
	for i := range covers {
		if covers[i].Size == "DEFAULT" {
			return &covers[i]
		}
	}
	if len(covers) > 0 {
		return &covers[0]
	}
	return nil
}

// CoverURL returns the Spotify CDN URL for a cover image file ID.
func CoverURL(fileID string) string {
	return "https://i.scdn.co/image/" + fileID
}

// ISRC returns the ISRC external ID for a track, or empty string if not present.
func ISRC(ids []ExternalID) string {
	for _, id := range ids {
		if id.Type == "isrc" {
			return id.ID
		}
	}
	return ""
}

// formatPriority defines preference order for audio formats (lower = better).
var formatPriority = map[string]int{
	"FLAC_FLAC":       0,
	"FLAC_FLAC_24BIT": 1,
	"OGG_VORBIS_320":  2,
	"OGG_VORBIS_160":  3,
	"OGG_VORBIS_96":   4,
	"AAC_320":         5,
	"MP3_320":         6,
	"MP3_256":         7,
	"AAC_160":         8,
	"MP3_160":         9,
	"MP4_128":         10,
	"MP3_96":          11,
	"AAC_48":          12,
	"AAC_24":          13,
	"XHE_AAC_24":      14,
	"XHE_AAC_16":      15,
	"XHE_AAC_12":      16,
}

// PreferAudioFile selects the best audio file from a list based on format quality.
// Returns nil if the list is empty.
func PreferAudioFile(files []AudioFile) *AudioFile {
	if len(files) == 0 {
		return nil
	}
	best := &files[0]
	bestPrio := priorityOf(best.Format)
	for i := 1; i < len(files); i++ {
		p := priorityOf(files[i].Format)
		if p < bestPrio {
			best = &files[i]
			bestPrio = p
		}
	}
	return best
}

func priorityOf(format string) int {
	if p, ok := formatPriority[format]; ok {
		return p
	}
	return 100 // unknown formats last
}
