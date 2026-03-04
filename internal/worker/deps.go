package worker

import (
	"github.com/halfdane/fetching/internal/spotify"
)

// MetadataFetcher wraps the CLI's metadata and audio fetch operations.
// *cli.Runner satisfies this interface.
type MetadataFetcher interface {
	FetchMetadata(uri string) ([]byte, error)
	FetchAudio(uri, fileID, outputPath string) error
}

// AudioTagger writes metadata and cover art into a downloaded audio file.
// *tagger.Tagger satisfies this interface.
type AudioTagger interface {
	TagTrack(path string, track *spotify.Track) error
	TagEpisode(path string, ep *spotify.Episode) error
}
