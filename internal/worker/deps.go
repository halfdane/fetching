package worker

import (
	"io"

	"github.com/halfdane/fetching/internal/credentials"
	"github.com/halfdane/fetching/internal/spotify"
)

// MetadataFetcher wraps the CLI's metadata and audio fetch operations.
// *cli.Runner satisfies this interface.
type MetadataFetcher interface {
	FetchMetadata(creds *credentials.Credentials, uri string) ([]byte, error)
	FetchAudio(creds *credentials.Credentials, uri, fileID string, w io.Writer) error
}

// CredentialProvider returns valid Spotify credentials, refreshing as needed.
// *credentials.Store satisfies this interface.
type CredentialProvider interface {
	Get() (*credentials.Credentials, error)
}

// AudioTagger writes metadata and cover art into a downloaded audio file.
// *tagger.Tagger satisfies this interface.
type AudioTagger interface {
	TagTrack(path string, track *spotify.Track) error
	TagEpisode(path string, ep *spotify.Episode) error
}
