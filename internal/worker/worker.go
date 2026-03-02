// Package worker processes download jobs from the queue sequentially.
package worker

import (
	"context"
	"log"
	"time"

	"github.com/halfdane/fetching/internal/cli"
	"github.com/halfdane/fetching/internal/credentials"
	"github.com/halfdane/fetching/internal/queue"
	"github.com/halfdane/fetching/internal/spotify"
	"github.com/halfdane/fetching/internal/storage"
)

// Worker pulls jobs from the queue and processes them.
type Worker struct {
	queue   *queue.Queue
	runner  *cli.Runner
	creds   *credentials.Store
	store   *storage.Storage
	pollInterval time.Duration
	concurrency  int
}

// New creates a worker with the given dependencies.
func New(q *queue.Queue, runner *cli.Runner, creds *credentials.Store, store *storage.Storage, concurrency int) *Worker {
	if concurrency < 1 {
		concurrency = 1
	}
	return &Worker{
		queue:        q,
		runner:       runner,
		creds:        creds,
		store:        store,
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

	// Step 3: For each track, fetch its metadata and download audio
	for _, uri := range trackURIs {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if err := w.downloadTrack(creds, uri); err != nil {
			log.Printf("  track %s failed: %v", uri, err)
			// Continue with remaining tracks rather than failing the whole job
			continue
		}
	}

	return nil
}

func (w *Worker) downloadTrack(creds *credentials.Credentials, trackURI string) error {
	// Fetch track metadata to get audio file IDs
	metaJSON, err := w.runner.FetchMetadata(creds, trackURI)
	if err != nil {
		return err
	}

	meta, err := spotify.ParseMetadata(metaJSON)
	if err != nil {
		return err
	}

	track, ok := meta.(*spotify.Track)
	if !ok {
		// Could be an episode
		ep, ok := meta.(*spotify.Episode)
		if !ok {
			return nil // skip unknown types
		}
		return w.downloadEpisode(creds, ep)
	}

	log.Printf("  processing track: %s", track)

	if len(track.AudioFiles) == 0 {
		log.Printf("  track %s has no audio files, skipping", trackURI)
		return nil
	}

	// Pick the first available audio file
	fileID := track.AudioFiles[0].FileID

	// Create a writer to the storage location
	writer, err := w.store.CreateTrackWriter(track)
	if err != nil {
		return err
	}
	defer writer.Close()

	log.Printf("  downloading %s - %s", track.Artists[0].Name, track.Name)
	return w.runner.FetchAudio(creds, trackURI, fileID, writer)
}

func (w *Worker) downloadEpisode(creds *credentials.Credentials, ep *spotify.Episode) error {
	if len(ep.AudioFiles) == 0 {
		log.Printf("  episode %s has no audio files, skipping", ep.URI)
		return nil
	}

	fileID := ep.AudioFiles[0].FileID

	writer, err := w.store.CreateEpisodeWriter(ep)
	if err != nil {
		return err
	}
	defer writer.Close()

	log.Printf("  downloading episode: %s", ep.Name)
	return w.runner.FetchAudio(creds, ep.URI, fileID, writer)
}
