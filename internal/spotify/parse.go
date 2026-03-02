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
