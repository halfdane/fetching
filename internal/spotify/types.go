// Package spotify defines domain types for Spotify metadata as returned
// by fetching-cli.
package spotify

import "strings"

// ItemType represents the kind of Spotify resource.
type ItemType string

const (
	TypeTrack    ItemType = "track"
	TypeAlbum    ItemType = "album"
	TypePlaylist ItemType = "playlist"
	TypeShow     ItemType = "show"
	TypeEpisode  ItemType = "episode"
)

// Artist is a minimal Spotify artist reference.
type Artist struct {
	URI  string `json:"uri"`
	Name string `json:"name"`
}

// ArtistWithRole extends Artist with a role field.
type ArtistWithRole struct {
	URI  string `json:"uri"`
	Name string `json:"name"`
	Role string `json:"role"`
}

// Cover represents an album/episode cover image.
type Cover struct {
	FileID string `json:"file_id"`
	Size   string `json:"size"`
	Width  int    `json:"width"`
	Height int    `json:"height"`
}

// ExternalID is an industry identifier like ISRC or UPC.
type ExternalID struct {
	Type string `json:"external_type"`
	ID   string `json:"id"`
}

// AlbumRef is a lightweight album reference embedded in track metadata.
type AlbumRef struct {
	URI         string       `json:"uri"`
	Name        string       `json:"name"`
	Artists     []Artist     `json:"artists"`
	Covers      []Cover      `json:"covers"`
	Date        string       `json:"date"`
	Label       string       `json:"label"`
	ExternalIDs []ExternalID `json:"external_ids"`
}

// AudioFile represents a downloadable audio file reference.
type AudioFile struct {
	FileID string `json:"file_id"`
	Format string `json:"format"`
}

// Extension returns the appropriate file extension for this audio format.
func (a AudioFile) Extension() string {
	switch {
	case strings.HasPrefix(a.Format, "OGG_VORBIS"):
		return ".ogg"
	case strings.HasPrefix(a.Format, "MP3"):
		return ".mp3"
	case strings.HasPrefix(a.Format, "AAC"), strings.HasPrefix(a.Format, "XHE_AAC"):
		return ".m4a"
	case strings.HasPrefix(a.Format, "MP4"):
		return ".m4a"
	case strings.HasPrefix(a.Format, "FLAC"):
		return ".flac"
	default:
		return ".ogg"
	}
}

// Disc groups tracks within an album.
type Disc struct {
	Number int      `json:"number"`
	Name   string   `json:"name"`
	Tracks []string `json:"tracks"` // track URIs
}

// Track holds metadata for a single Spotify track.
type Track struct {
	Type                  ItemType         `json:"type"`
	URI                   string           `json:"uri"`
	Name                  string           `json:"name"`
	Album                 AlbumRef         `json:"album"`
	Artists               []Artist         `json:"artists"`
	ArtistsWithRole       []ArtistWithRole `json:"artists_with_role"`
	Number                int              `json:"number"`
	DiscNumber            int              `json:"disc_number"`
	DurationMS            int              `json:"duration_ms"`
	Popularity            int              `json:"popularity"`
	IsExplicit            bool             `json:"is_explicit"`
	ExternalIDs           []ExternalID     `json:"external_ids"`
	AudioFiles            []AudioFile      `json:"files"`
	Alternatives          []string         `json:"alternatives"`
	OriginalTitle         string           `json:"original_title"`
	VersionTitle          string           `json:"version_title"`
	LanguageOfPerformance []string         `json:"language_of_performance"`
	HasLyrics             bool             `json:"has_lyrics"`
}

// Album holds metadata for a Spotify album.
type Album struct {
	Type        ItemType     `json:"type"`
	URI         string       `json:"uri"`
	Name        string       `json:"name"`
	Artists     []Artist     `json:"artists"`
	AlbumType   string       `json:"album_type"`
	Label       string       `json:"label"`
	Date        string       `json:"date"`
	Popularity  int          `json:"popularity"`
	Covers      []Cover      `json:"covers"`
	ExternalIDs []ExternalID `json:"external_ids"`
	Discs       []Disc       `json:"discs"`
}

// Playlist holds metadata for a Spotify playlist.
type Playlist struct {
	Type            ItemType `json:"type"`
	URI             string   `json:"uri"`
	Name            string   `json:"name"`
	Description     string   `json:"description"`
	Length          int      `json:"length"`
	TrackURIs       []string `json:"track_uris"`
	IsCollaborative bool     `json:"is_collaborative"`
}

// Show holds metadata for a Spotify podcast show.
type Show struct {
	Type        ItemType `json:"type"`
	URI         string   `json:"uri"`
	Name        string   `json:"name"`
	Publisher   string   `json:"publisher"`
	Description string   `json:"description"`
	EpisodeURIs []string `json:"episode_uris"`
}

// Episode holds metadata for a Spotify podcast episode.
type Episode struct {
	Type        ItemType    `json:"type"`
	URI         string      `json:"uri"`
	Name        string      `json:"name"`
	ShowURI     string      `json:"show_uri"`
	ShowName    string      `json:"show_name"`
	Description string      `json:"description"`
	DurationMS  int         `json:"duration_ms"`
	Number      int         `json:"number"`
	PublishTime string      `json:"publish_time"`
	Language    string      `json:"language"`
	IsExplicit  bool        `json:"is_explicit"`
	Covers      []Cover     `json:"covers"`
	AudioFiles  []AudioFile `json:"audio_files"`
}
