// Package worker processes fetch jobs from the queue sequentially.
package worker

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"time"

	"github.com/halfdane/fetching/internal/progress"
	"github.com/halfdane/fetching/internal/queue"
	"github.com/halfdane/fetching/internal/spotify"
	"github.com/halfdane/fetching/internal/storage"
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
	runner       MetadataFetcher
	store        *storage.Storage
	tagger       AudioTagger
	progress     *progress.Store
	pollInterval time.Duration
	concurrency  int
}

// New creates a worker with the given dependencies.
func New(q *queue.Queue, runner MetadataFetcher, store *storage.Storage, tgr AudioTagger, prog *progress.Store, concurrency int) *Worker {
	if concurrency < 1 {
		concurrency = 1
	}
	return &Worker{
		queue:        q,
		runner:       runner,
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
	notify := w.queue.Notify()
	// sem limits the number of concurrently running jobs.
	sem := make(chan struct{}, w.concurrency)

	for {
		select {
		case <-ctx.Done():
			// Drain the semaphore so all in-flight jobs have finished.
			for i := 0; i < w.concurrency; i++ {
				sem <- struct{}{}
			}
			return ctx.Err()
		default:
		}

		job, err := w.queue.Next()
		if err != nil {
			slog.Error("error getting next job", "err", err)
			if oneShot {
				return err
			}
			w.waitForWork(ctx, notify)
			continue
		}

		if job == nil {
			if oneShot {
				// Wait for all in-flight jobs to finish before returning.
				for i := 0; i < w.concurrency; i++ {
					sem <- struct{}{}
				}
				return nil // all done
			}
			w.waitForWork(ctx, notify)
			continue
		}

		slog.Info("processing job", "id", job.ID, "uri", job.SpotifyURI)
		sem <- struct{}{} // acquire slot
		go func(j *queue.Job) {
			defer func() { <-sem }() // release slot
			if err := w.processJob(ctx, j); err != nil {
				slog.Error("job failed", "id", j.ID, "err", err)
				_ = w.queue.Fail(j.ID, err.Error())
			} else {
				slog.Info("job completed", "id", j.ID)
				_ = w.queue.Complete(j.ID)
			}
		}(job)
	}
}

// waitForWork blocks until either new work is signalled, the poll interval
// elapses (as a fallback for delayed re-enqueues), or the context is cancelled.
func (w *Worker) waitForWork(ctx context.Context, notify <-chan struct{}) {
	select {
	case <-notify:
	case <-time.After(w.pollInterval):
	case <-ctx.Done():
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

	// Step 1: Fetch metadata for the URI
	metaJSON, err := w.runner.FetchMetadata(job.SpotifyURI)
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

		res, err := w.fetchTrack(ctx, job.ID, uri, job.FallbackQuality)
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

func (w *Worker) fetchTrack(ctx context.Context, jobID int64, trackURI string, fallbackQuality bool) (*fetchResult, error) {
	// Fetch track metadata to get audio file IDs
	metaJSON, err := w.runner.FetchMetadata(trackURI)
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
		return w.fetchEpisode(ctx, jobID, ep)
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
		altJSON, err := w.runner.FetchMetadata(altURI)
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

		candOutPath, prepErr := w.store.PrepareTrackPath(track, af.Extension())
		if prepErr != nil {
			lastErr = prepErr
			break
		}
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
				return w.runner.FetchAudio(srcURI, af.FileID, candOutPath)
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
			os.Remove(candOutPath)
			return nil, err
		}

		// Remove the partial/empty file so a later os.Stat skip-check won't
		// falsely believe the track is already downloaded.
		os.Remove(candOutPath)

		lastErr = err
		if !fallbackQuality || i == len(sorted)-1 {
			break
		}
		slog.Info("trying next quality level", "uri", trackURI, "format", af.Format)
	}

	return nil, fmt.Errorf("all candidates failed for %s: %w", trackURI, lastErr)
}

func (w *Worker) fetchEpisode(ctx context.Context, jobID int64, ep *spotify.Episode) (*fetchResult, error) {
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
	outPath, prepErr := w.store.PrepareEpisodePath(ep, af.Extension())
	if prepErr != nil {
		return nil, prepErr
	}
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
			return w.runner.FetchAudio(ep.URI, af.FileID, outPath)
		})
	if err != nil {
		// Remove partial file so the next attempt won't be falsely skipped.
		os.Remove(outPath)
		return nil, err
	}
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
