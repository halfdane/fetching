// Package worker processes download jobs from the queue sequentially.
package worker

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/halfdane/fetching/internal/cli"
	"github.com/halfdane/fetching/internal/cover"
	"github.com/halfdane/fetching/internal/credentials"
	"github.com/halfdane/fetching/internal/playlist"
	"github.com/halfdane/fetching/internal/queue"
	"github.com/halfdane/fetching/internal/spotify"
	"github.com/halfdane/fetching/internal/storage"
	"github.com/halfdane/fetching/internal/tagger"
)

// trackRetryDelays mirrors the queue retry schedule for per-track transient failures.
var trackRetryDelays = []time.Duration{
	1 * time.Second,
	5 * time.Second,
	15 * time.Second,
}

// withRetry calls fn up to 1+len(trackRetryDelays) times, sleeping between
// attempts. Returns the last error if all attempts fail.
func withRetry(label string, fn func() error) error {
	if err := fn(); err == nil {
		return nil
	} else {
		log.Printf("  %s failed (attempt 1): %v", label, err)
	}
	for i, d := range trackRetryDelays {
		time.Sleep(d)
		if err := fn(); err == nil {
			return nil
		} else {
			log.Printf("  %s failed (attempt %d): %v", label, i+2, err)
		}
	}
	return fmt.Errorf("all %d attempts failed", 1+len(trackRetryDelays))
}

// Worker pulls jobs from the queue and processes them.
type Worker struct {
	queue        *queue.Queue
	runner       *cli.Runner
	creds        *credentials.Store
	store        *storage.Storage
	tagger       *tagger.Tagger
	pollInterval time.Duration
	concurrency  int
}

// New creates a worker with the given dependencies.
func New(q *queue.Queue, runner *cli.Runner, creds *credentials.Store, store *storage.Storage, tgr *tagger.Tagger, concurrency int) *Worker {
	if concurrency < 1 {
		concurrency = 1
	}
	return &Worker{
		queue:        q,
		runner:       runner,
		creds:        creds,
		store:        store,
		tagger:       tgr,
		pollInterval: 2 * time.Second,
		concurrency:  concurrency,
	}
}

// Run starts processing jobs until the context is cancelled.
// When oneShot is true, it processes all pending jobs and returns.
func (w *Worker) Run(ctx context.Context, oneShot bool) error {
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		job, err := w.queue.Next()
		if err != nil {
			log.Printf("error getting next job: %v", err)
			if oneShot {
				return err
			}
			time.Sleep(w.pollInterval)
			continue
		}

		if job == nil {
			if oneShot {
				return nil // all done
			}
			time.Sleep(w.pollInterval)
			continue
		}

		log.Printf("processing job %d: %s", job.ID, job.SpotifyURI)
		if err := w.processJob(ctx, job); err != nil {
			log.Printf("job %d failed: %v", job.ID, err)
			_ = w.queue.Fail(job.ID, err.Error())
		} else {
			log.Printf("job %d completed", job.ID)
			_ = w.queue.Complete(job.ID)
		}
	}
}

// downloadResult holds the outcome of downloading a single track or episode.
type downloadResult struct {
	Path     string
	Duration int // seconds
	Artist   string
	Title    string
	// CoverURL is the Spotify CDN URL for this item's cover (LARGE preferred).
	CoverURL string
}

func (w *Worker) processJob(ctx context.Context, job *queue.Job) error {
	creds, err := w.creds.Get()
	if err != nil {
		return err
	}

	// Step 1: Fetch metadata for the URI
	metaJSON, err := w.runner.FetchMetadata(creds, job.SpotifyURI)
	if err != nil {
		return err
	}

	meta, err := spotify.ParseMetadata(metaJSON)
	if err != nil {
		return err
	}

	// Step 2: Resolve individual track URIs
	trackURIs := spotify.TrackURIs(meta)

	// Step 3: For each track, fetch its metadata, download, and tag
	var results []downloadResult
	for _, uri := range trackURIs {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		var res *downloadResult
		if err := withRetry(uri, func() error {
			var e error
			res, e = w.downloadTrack(creds, uri)
			return e
		}); err != nil {
			log.Printf("  track %s permanently failed: %v", uri, err)
			continue
		}
		if res != nil {
			results = append(results, *res)
		}
	}

	// Step 4: Generate M3U8 playlist and cover
	w.generatePlaylistAndCover(meta, results)

	return nil
}

func (w *Worker) generatePlaylistAndCover(meta any, results []downloadResult) {
	if len(results) == 0 {
		return
	}

	switch v := meta.(type) {
	case *Album:
		w.generateAlbumAssets(v, results)
	case *Playlist:
		w.generatePlaylistAssets(v, results)
	case *Show:
		w.generateShowAssets(v, results)
	case *Track:
		// Single track — no playlist to generate
	case *Episode:
		// Single episode — no playlist to generate
	}
}

// Import types used in switch (they're from spotify package but we reference via meta).
type (
	Album    = spotify.Album
	Playlist = spotify.Playlist
	Show     = spotify.Show
	Track    = spotify.Track
	Episode  = spotify.Episode
)

func (w *Worker) generateAlbumAssets(album *Album, results []downloadResult) {
	if len(results) == 0 {
		return
	}

	artist := "Unknown Artist"
	if len(album.Artists) > 0 {
		artist = album.Artists[0].Name
	}
	dir := w.store.AlbumDir(artist, album.Name)

	// M3U8
	entries := resultsToEntries(results)
	m3u8Meta := playlist.Metadata{
		"name":        album.Name,
		"artist":      artist,
		"date":        album.Date,
		"label":       album.Label,
		"spotify_uri": album.URI,
	}
	if upc := spotify.UPC(album.ExternalIDs); upc != "" {
		m3u8Meta["upc"] = upc
	}

	dest := dir + "/" + storage.Sanitize(album.Name) + ".m3u8"
	if err := playlist.WriteM3U8(dest, entries, m3u8Meta); err != nil {
		log.Printf("  warning: failed to write album M3U8: %v", err)
	} else {
		log.Printf("  wrote album playlist: %s", dest)
	}

	// Cover (LARGE)
	if err := cover.SaveAlbumCover(dir, album.Covers); err != nil {
		log.Printf("  warning: failed to save album cover: %v", err)
	} else {
		log.Printf("  saved album cover: %s/cover.jpg", dir)
	}
}

func (w *Worker) generateShowAssets(show *Show, results []downloadResult) {
	if len(results) == 0 {
		return
	}

	dir := w.store.ShowDir(show.Name)

	entries := resultsToEntries(results)
	m3u8Meta := playlist.Metadata{
		"name":        show.Name,
		"publisher":   show.Publisher,
		"spotify_uri": show.URI,
	}

	dest := dir + "/" + storage.Sanitize(show.Name) + ".m3u8"
	if err := playlist.WriteM3U8(dest, entries, m3u8Meta); err != nil {
		log.Printf("  warning: failed to write show M3U8: %v", err)
	} else {
		log.Printf("  wrote show playlist: %s", dest)
	}

	// Shows don't have covers at the Show level in our types,
	// but episodes do — use the first episode's cover if available.
	if results[0].CoverURL != "" {
		if err := cover.SavePlaylistCover(dir, []string{results[0].CoverURL}); err != nil {
			log.Printf("  warning: failed to save show cover: %v", err)
		} else {
			log.Printf("  saved show cover: %s/cover.jpg", dir)
		}
	}
}

func (w *Worker) generatePlaylistAssets(pl *Playlist, results []downloadResult) {
	if len(results) == 0 {
		return
	}

	dir := w.store.PlaylistDir(pl.Name)

	entries := resultsToEntries(results)
	m3u8Meta := playlist.Metadata{
		"name":        pl.Name,
		"description": pl.Description,
		"spotify_uri": pl.URI,
	}

	dest := dir + "/" + storage.Sanitize(pl.Name) + ".m3u8"
	if err := playlist.WriteM3U8(dest, entries, m3u8Meta); err != nil {
		log.Printf("  warning: failed to write playlist M3U8: %v", err)
	} else {
		log.Printf("  wrote playlist: %s", dest)
	}

	// Composite cover from unique track covers
	var coverURLs []string
	seen := make(map[string]bool)
	for _, r := range results {
		if r.CoverURL != "" && !seen[r.CoverURL] {
			seen[r.CoverURL] = true
			coverURLs = append(coverURLs, r.CoverURL)
			if len(coverURLs) >= 4 {
				break
			}
		}
	}

	if len(coverURLs) > 0 {
		if err := cover.SavePlaylistCover(dir, coverURLs); err != nil {
			log.Printf("  warning: failed to generate playlist cover: %v", err)
		} else {
			log.Printf("  saved playlist cover: %s/cover.jpg", dir)
		}
	}
}

func resultsToEntries(results []downloadResult) []playlist.TrackEntry {
	entries := make([]playlist.TrackEntry, len(results))
	for i, r := range results {
		entries[i] = playlist.TrackEntry{
			Path:        r.Path,
			DurationSec: r.Duration,
			Artist:      r.Artist,
			Title:       r.Title,
		}
	}
	return entries
}

func (w *Worker) downloadTrack(creds *credentials.Credentials, trackURI string) (*downloadResult, error) {
	// Fetch track metadata to get audio file IDs
	metaJSON, err := w.runner.FetchMetadata(creds, trackURI)
	if err != nil {
		return nil, err
	}

	meta, err := spotify.ParseMetadata(metaJSON)
	if err != nil {
		return nil, err
	}

	track, ok := meta.(*spotify.Track)
	if !ok {
		// Could be an episode
		ep, ok := meta.(*spotify.Episode)
		if !ok {
			return nil, fmt.Errorf("unrecognised metadata type for %s", trackURI)
		}
		return w.downloadEpisode(creds, ep)
	}

	af := spotify.PreferAudioFile(track.AudioFiles)
	if af == nil {
		return nil, fmt.Errorf("track %s has no audio files", trackURI)
	}

	log.Printf("  selected format %s for %s", af.Format, trackURI)

	// Create a writer to the storage location
	outPath, writer, err := w.store.CreateTrackWriter(track, af.Extension())
	if err != nil {
		return nil, err
	}

	artist := "Unknown"
	if len(track.Artists) > 0 {
		artist = track.Artists[0].Name
	}

	log.Printf("  downloading %s - %s", artist, track.Name)
	if err := w.runner.FetchAudio(creds, trackURI, af.FileID, writer); err != nil {
		writer.Close()
		return nil, err
	}
	writer.Close()

	// Tag the downloaded file with metadata and cover art
	log.Printf("  tagging %s - %s", artist, track.Name)
	if err := w.tagger.TagTrack(outPath, track); err != nil {
		log.Printf("  warning: tagging failed for %s: %v", trackURI, err)
	}

	// Build cover URL for playlist composite
	var coverURL string
	if c := spotify.LargeCover(track.Album.Covers); c != nil {
		coverURL = spotify.CoverURL(c.FileID)
	}

	return &downloadResult{
		Path:     outPath,
		Duration: track.DurationMS / 1000,
		Artist:   artist,
		Title:    track.Name,
		CoverURL: coverURL,
	}, nil
}

func (w *Worker) downloadEpisode(creds *credentials.Credentials, ep *spotify.Episode) (*downloadResult, error) {
	af := spotify.PreferAudioFile(ep.AudioFiles)
	if af == nil {
		return nil, fmt.Errorf("episode %s has no audio files", ep.URI)
	}

	log.Printf("  selected format %s for %s", af.Format, ep.URI)

	outPath, writer, err := w.store.CreateEpisodeWriter(ep, af.Extension())
	if err != nil {
		return nil, err
	}

	log.Printf("  downloading episode: %s", ep.Name)
	if err := w.runner.FetchAudio(creds, ep.URI, af.FileID, writer); err != nil {
		writer.Close()
		return nil, err
	}
	writer.Close()

	// Tag the downloaded episode with metadata and cover art
	log.Printf("  tagging episode: %s", ep.Name)
	if err := w.tagger.TagEpisode(outPath, ep); err != nil {
		log.Printf("  warning: tagging failed for %s: %v", ep.URI, err)
	}

	var coverURL string
	if c := spotify.LargeCover(ep.Covers); c != nil {
		coverURL = spotify.CoverURL(c.FileID)
	}

	return &downloadResult{
		Path:     outPath,
		Duration: ep.DurationMS / 1000,
		Artist:   ep.ShowName,
		Title:    ep.Name,
		CoverURL: coverURL,
	}, nil
}
