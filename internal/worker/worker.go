// Package worker processes fetch jobs from the queue sequentially.
package worker

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"time"

	"github.com/halfdane/fetching/internal/cli"
	"github.com/halfdane/fetching/internal/cover"
	"github.com/halfdane/fetching/internal/credentials"
	"github.com/halfdane/fetching/internal/playlist"
	"github.com/halfdane/fetching/internal/progress"
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
func withRetry(ctx context.Context, label string, onRetry func(retryAttempt, retryMax int, wait time.Duration, lastErr error), fn func() error) error {
	if err := fn(); err == nil {
		return nil
	} else {
		slog.Warn("attempt failed", "label", label, "attempt", 1, "err", err)
		lastErr := err
		for i, d := range trackRetryDelays {
			retryAttempt := i + 1
			retryMax := len(trackRetryDelays)
			if onRetry != nil {
				onRetry(retryAttempt, retryMax, d, lastErr)
			}
			select {
			case <-time.After(d):
			case <-ctx.Done():
				return ctx.Err()
			}
			if err := fn(); err == nil {
				return nil
			} else {
				lastErr = err
				slog.Warn("attempt failed", "label", label, "attempt", i+2, "err", err)
			}
		}
		return fmt.Errorf("all %d attempts failed: %w", 1+len(trackRetryDelays), lastErr)
	}
}

// Worker pulls jobs from the queue and processes them.
type Worker struct {
	queue        *queue.Queue
	runner       *cli.Runner
	creds        *credentials.Store
	store        *storage.Storage
	tagger       *tagger.Tagger
	progress     *progress.Store
	pollInterval time.Duration
	concurrency  int
}

// New creates a worker with the given dependencies.
func New(q *queue.Queue, runner *cli.Runner, creds *credentials.Store, store *storage.Storage, tgr *tagger.Tagger, prog *progress.Store, concurrency int) *Worker {
	if concurrency < 1 {
		concurrency = 1
	}
	return &Worker{
		queue:        q,
		runner:       runner,
		creds:        creds,
		store:        store,
		tagger:       tgr,
		progress:     prog,
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
			slog.Error("error getting next job", "err", err)
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

		slog.Info("processing job", "id", job.ID, "uri", job.SpotifyURI)
		if err := w.processJob(ctx, job); err != nil {
			slog.Error("job failed", "id", job.ID, "err", err)
			_ = w.queue.Fail(job.ID, err.Error())
		} else {
			slog.Info("job completed", "id", job.ID)
			_ = w.queue.Complete(job.ID)
		}
	}
}

// fetchResult holds the outcome of fetching a single track or episode.
type fetchResult struct {
	Path     string
	Duration int // seconds
	Artist   string
	Title    string
	// CoverURL is the Spotify CDN URL for this item's cover (LARGE preferred).
	CoverURL string
}

func (w *Worker) processJob(ctx context.Context, job *queue.Job) error {
	if w.progress != nil {
		w.progress.UpsertSubmitted(job.ID, job.SpotifyURI)
	}

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

	if w.progress != nil {
		kind := "collection"
		title := job.SpotifyURI
		coverURL := ""
		switch v := meta.(type) {
		case *spotify.Album:
			kind = "album"
			title = v.Name
			if c := spotify.LargeCover(v.Covers); c != nil {
				coverURL = spotify.CoverURL(c.FileID)
			}
		case *spotify.Playlist:
			kind = "playlist"
			title = v.Name
		case *spotify.Show:
			kind = "show"
			title = v.Name
		case *spotify.Track:
			kind = "track"
			title = v.Name
			if c := spotify.LargeCover(v.Album.Covers); c != nil {
				coverURL = spotify.CoverURL(c.FileID)
			}
		case *spotify.Episode:
			kind = "show"
			title = v.ShowName
			if c := spotify.LargeCover(v.Covers); c != nil {
				coverURL = spotify.CoverURL(c.FileID)
			}
		}
		w.progress.SetCollectionMeta(job.ID, kind, title, coverURL, len(trackURIs))
		for _, uri := range trackURIs {
			w.progress.SetTrackQueued(job.ID, uri)
		}
	}

	// Step 3: For each track, fetch its metadata, fetch audio, and tag
	var results []fetchResult
	for _, uri := range trackURIs {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if w.progress != nil {
			w.progress.UpdateTrack(job.ID, uri, func(t *progress.TrackView) {
				t.Status = progress.TrackResolvingMetadata
				t.ErrorMessage = ""
				t.RetryAttempt = 0
				t.RetryMax = len(trackRetryDelays)
			})
		}

		res, err := w.fetchTrack(ctx, job.ID, creds, uri, job.FallbackQuality)
		if err != nil {
			if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
				return err
			}
			slog.Error("track permanently failed", "uri", uri, "err", err)
			if w.progress != nil {
				w.progress.UpdateTrack(job.ID, uri, func(t *progress.TrackView) {
					t.Status = progress.TrackFailed
					t.ErrorMessage = err.Error()
				})
			}
			continue
		}
		if res != nil {
			if w.progress != nil {
				w.progress.UpdateTrack(job.ID, uri, func(t *progress.TrackView) {
					t.Status = progress.TrackDone
					t.Title = res.Title
					t.DurationSec = res.Duration
					t.ErrorMessage = ""
				})
			}
			results = append(results, *res)
		}
	}

	// Step 4: Generate M3U8 playlist and cover
	w.generatePlaylistAndCover(meta, results)

	if w.progress != nil {
		w.progress.MarkCollectionTerminal(job.ID)
	}

	return nil
}

func (w *Worker) generatePlaylistAndCover(meta any, results []fetchResult) {
	if len(results) == 0 {
		return
	}

	switch v := meta.(type) {
	case *spotify.Album:
		w.generateAlbumAssets(v, results)
	case *spotify.Playlist:
		w.generatePlaylistAssets(v, results)
	case *spotify.Show:
		w.generateShowAssets(v, results)
	case *spotify.Track:
		// Single track — no playlist to generate
	case *spotify.Episode:
		// Single episode — no playlist to generate
	}
}

func (w *Worker) generateAlbumAssets(album *spotify.Album, results []fetchResult) {
	if len(results) == 0 {
		return
	}

	artist := "Unknown Artist"
	if len(album.Artists) > 0 {
		artist = album.Artists[0].Name
	}
	// Derive the album dir from where the first track actually landed,
	// so it's always consistent with the path template.
	dir := filepath.Dir(results[0].Path)

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
		slog.Warn("failed to write album M3U8", "err", err)
	} else {
		slog.Info("wrote album playlist", "path", dest)
	}

	// Cover (LARGE)
	if err := cover.SaveAlbumCover(dir, album.Covers); err != nil {
		slog.Warn("failed to save album cover", "err", err)
	} else {
		slog.Info("saved album cover", "dir", dir)
	}
}

func (w *Worker) generateShowAssets(show *spotify.Show, results []fetchResult) {
	if len(results) == 0 {
		return
	}

	// Derive the show dir from where the first episode actually landed.
	dir := filepath.Dir(results[0].Path)

	entries := resultsToEntries(results)
	m3u8Meta := playlist.Metadata{
		"name":        show.Name,
		"publisher":   show.Publisher,
		"spotify_uri": show.URI,
	}

	dest := dir + "/" + storage.Sanitize(show.Name) + ".m3u8"
	if err := playlist.WriteM3U8(dest, entries, m3u8Meta); err != nil {
		slog.Warn("failed to write show M3U8", "err", err)
	} else {
		slog.Info("wrote show playlist", "path", dest)
	}

	// Shows don't have covers at the Show level in our types,
	// but episodes do — use the first episode's cover if available.
	if results[0].CoverURL != "" {
		if err := cover.SavePlaylistCover(dir, []string{results[0].CoverURL}); err != nil {
			slog.Warn("failed to save show cover", "err", err)
		} else {
			slog.Info("saved show cover", "dir", dir)
		}
	}
}

func (w *Worker) generatePlaylistAssets(pl *spotify.Playlist, results []fetchResult) {
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
		slog.Warn("failed to write playlist M3U8", "err", err)
	} else {
		slog.Info("wrote playlist", "path", dest)
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
			slog.Warn("failed to generate playlist cover", "err", err)
		} else {
			slog.Info("saved playlist cover", "dir", dir)
		}
	}
}

func resultsToEntries(results []fetchResult) []playlist.TrackEntry {
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

func (w *Worker) fetchTrack(ctx context.Context, jobID int64, creds *credentials.Credentials, trackURI string, fallbackQuality bool) (*fetchResult, error) {
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
		return w.fetchEpisode(ctx, jobID, creds, ep)
	}

	artist := "Unknown"
	if len(track.Artists) > 0 {
		artist = track.Artists[0].Name
	}
	// Update title/duration as soon as metadata is known, so it's visible
	// even if the fetch fails (e.g. track has no audio files).
	if w.progress != nil {
		w.progress.UpdateTrack(jobID, trackURI, func(t *progress.TrackView) {
			t.Title = track.Name
			t.DurationSec = track.DurationMS / 1000
		})
	}

	// Build candidate pool from primary track + all alternatives, then sort
	// by quality. Track which URI owns each file because FetchAudio needs the
	// owning URI, not always the original.
	candidates := make([]spotify.CandidateFile, 0, len(track.AudioFiles))
	for _, f := range track.AudioFiles {
		candidates = append(candidates, spotify.CandidateFile{TrackURI: trackURI, File: f})
	}
	for _, altURI := range track.Alternatives {
		altJSON, err := w.runner.FetchMetadata(creds, altURI)
		if err != nil {
			slog.Warn("failed to fetch alternative", "uri", altURI, "err", err)
			continue
		}
		altMeta, err := spotify.ParseMetadata(altJSON)
		if err != nil {
			slog.Warn("failed to parse alternative", "uri", altURI, "err", err)
			continue
		}
		if alt, ok := altMeta.(*spotify.Track); ok {
			for _, f := range alt.AudioFiles {
				candidates = append(candidates, spotify.CandidateFile{TrackURI: altURI, File: f})
			}
		}
	}
	sorted := spotify.SortedCandidates(candidates)
	if len(sorted) == 0 {
		return nil, fmt.Errorf("track %s has no audio files (checked %d alternatives)", trackURI, len(track.Alternatives))
	}

	var coverURL string
	if c := spotify.LargeCover(track.Album.Covers); c != nil {
		coverURL = spotify.CoverURL(c.FileID)
	}

	// Skip if already fetched (check against the best candidate's path).
	outPath := w.store.TrackPath(track, sorted[0].File.Extension())
	if _, err := os.Stat(outPath); err == nil {
		slog.Info("skipping (already fetched)", "artist", artist, "title", track.Name)
		if w.progress != nil {
			w.progress.UpdateTrack(jobID, trackURI, func(t *progress.TrackView) {
				t.Title = track.Name
				t.DurationSec = track.DurationMS / 1000
				t.Status = progress.TrackAlreadyPresent
				t.ErrorMessage = ""
			})
		}
		return &fetchResult{
			Path:     outPath,
			Duration: track.DurationMS / 1000,
			Artist:   artist,
			Title:    track.Name,
			CoverURL: coverURL,
		}, nil
	}

	// Outer loop: try each candidate in quality order. Each candidate gets its
	// own inner retry loop. We advance to the next candidate only when all
	// retries are exhausted AND fallbackQuality is enabled.
	var lastErr error
	for i, cand := range sorted {
		af := cand.File
		srcURI := cand.TrackURI
		if srcURI != trackURI {
			slog.Info("using alternative URI", "alt", srcURI, "track", trackURI, "format", af.Format)
		}
		slog.Info("selected format", "format", af.Format, "uri", trackURI)

		if w.progress != nil {
			w.progress.UpdateTrack(jobID, trackURI, func(t *progress.TrackView) {
				t.Status = progress.TrackFetchingAudio
				t.RetryAttempt = 0
				t.RetryMax = len(trackRetryDelays)
				t.ErrorMessage = ""
			})
		}

		candOutPath := w.store.TrackPath(track, af.Extension())
		err := withRetry(ctx, fmt.Sprintf("%s [%s]", trackURI, af.Format),
			func(retryAttempt, retryMax int, wait time.Duration, retryErr error) {
				if w.progress != nil {
					w.progress.UpdateTrack(jobID, trackURI, func(t *progress.TrackView) {
						t.Status = progress.TrackRetryWaiting
						t.RetryAttempt = retryAttempt
						t.RetryMax = retryMax
						t.ErrorMessage = retryErr.Error()
					})
				}
			},
			func() error {
				_, wr, err := w.store.CreateTrackWriter(track, af.Extension())
				if err != nil {
					return err
				}
				fetchErr := w.runner.FetchAudio(creds, srcURI, af.FileID, wr)
				wr.Close()
				return fetchErr
			})

		if err == nil {
			slog.Info("tagging track", "artist", artist, "title", track.Name)
			if err := w.tagger.TagTrack(candOutPath, track); err != nil {
				slog.Warn("tagging failed", "uri", trackURI, "err", err)
			}
			return &fetchResult{
				Path:     candOutPath,
				Duration: track.DurationMS / 1000,
				Artist:   artist,
				Title:    track.Name,
				CoverURL: coverURL,
			}, nil
		}

		if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			return nil, err
		}

		lastErr = err
		if !fallbackQuality || i == len(sorted)-1 {
			break
		}
		slog.Info("trying next quality level", "uri", trackURI, "format", af.Format)
	}

	return nil, fmt.Errorf("all candidates failed for %s: %w", trackURI, lastErr)
}

func (w *Worker) fetchEpisode(ctx context.Context, jobID int64, creds *credentials.Credentials, ep *spotify.Episode) (*fetchResult, error) {
	// Update title/duration as soon as metadata is known, so it's visible
	// even if the fetch fails (e.g. episode has no audio files).
	if w.progress != nil {
		w.progress.UpdateTrack(jobID, ep.URI, func(t *progress.TrackView) {
			t.Title = ep.Name
			t.DurationSec = ep.DurationMS / 1000
		})
	}

	af := spotify.PreferAudioFile(ep.AudioFiles)
	if af == nil {
		return nil, fmt.Errorf("episode %s has no audio files", ep.URI)
	}
	if w.progress != nil {
		w.progress.UpdateTrack(jobID, ep.URI, func(t *progress.TrackView) {
			t.Status = progress.TrackFetchingAudio
			t.ErrorMessage = ""
		})
	}

	var coverURL string
	if c := spotify.LargeCover(ep.Covers); c != nil {
		coverURL = spotify.CoverURL(c.FileID)
	}

	// Skip if already fetched.
	outPath := w.store.EpisodePath(ep, af.Extension())
	if _, err := os.Stat(outPath); err == nil {
		slog.Info("skipping episode (already fetched)", "title", ep.Name)
		if w.progress != nil {
			w.progress.UpdateTrack(jobID, ep.URI, func(t *progress.TrackView) {
				t.Title = ep.Name
				t.DurationSec = ep.DurationMS / 1000
				t.Status = progress.TrackAlreadyPresent
				t.ErrorMessage = ""
			})
		}
		return &fetchResult{
			Path:     outPath,
			Duration: ep.DurationMS / 1000,
			Artist:   ep.ShowName,
			Title:    ep.Name,
			CoverURL: coverURL,
		}, nil
	}

	slog.Info("fetching episode", "title", ep.Name)
	err := withRetry(ctx, fmt.Sprintf("episode %s", ep.URI),
		func(retryAttempt, retryMax int, wait time.Duration, retryErr error) {
			if w.progress != nil {
				w.progress.UpdateTrack(jobID, ep.URI, func(t *progress.TrackView) {
					t.Status = progress.TrackRetryWaiting
					t.RetryAttempt = retryAttempt
					t.RetryMax = retryMax
					t.ErrorMessage = retryErr.Error()
				})
			}
		},
		func() error {
			_, wr, err := w.store.CreateEpisodeWriter(ep, af.Extension())
			if err != nil {
				return err
			}
			fetchErr := w.runner.FetchAudio(creds, ep.URI, af.FileID, wr)
			wr.Close()
			return fetchErr
		})
	if err != nil {
		return nil, err
	}

	// Tag the fetched episode with metadata and cover art.
	slog.Info("tagging episode", "title", ep.Name)
	if err := w.tagger.TagEpisode(outPath, ep); err != nil {
		slog.Warn("tagging failed", "uri", ep.URI, "err", err)
	}

	return &fetchResult{
		Path:     outPath,
		Duration: ep.DurationMS / 1000,
		Artist:   ep.ShowName,
		Title:    ep.Name,
		CoverURL: coverURL,
	}, nil
}
