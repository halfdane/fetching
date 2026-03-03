package worker

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/halfdane/fetching/internal/credentials"
	"github.com/halfdane/fetching/internal/progress"
	"github.com/halfdane/fetching/internal/queue"
	"github.com/halfdane/fetching/internal/spotify"
	"github.com/halfdane/fetching/internal/storage"
)

// TestWithRetry_SucceedsFirstAttempt verifies the happy path.
func TestWithRetry_SucceedsFirstAttempt(t *testing.T) {
	calls := 0
	err := withRetry(context.Background(), "test", nil, func() error {
		calls++
		return nil
	})
	if err != nil {
		t.Errorf("expected nil, got %v", err)
	}
	if calls != 1 {
		t.Errorf("expected 1 call, got %d", calls)
	}
}

// TestWithRetry_ReturnsLastErrorAfterAllAttempts verifies all attempts are tried
// and the last error is wrapped and returned.
func TestWithRetry_ReturnsLastErrorAfterAllAttempts(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{1 * time.Millisecond, 1 * time.Millisecond}
	defer func() { trackRetryDelays = origDelays }()

	calls := 0
	err := withRetry(context.Background(), "test", nil, func() error {
		calls++
		return errors.New("boom")
	})
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	wantCalls := 1 + len(trackRetryDelays)
	if calls != wantCalls {
		t.Errorf("expected %d calls, got %d", wantCalls, calls)
	}
}

// TestWithRetry_ContextCancelledDuringSleep verifies that cancelling the context
// during a retry sleep interrupts the wait promptly and returns ctx.Err().
func TestWithRetry_ContextCancelledDuringSleep(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{5 * time.Second, 10 * time.Second} // long enough to reliably cancel
	defer func() { trackRetryDelays = origDelays }()

	ctx, cancel := context.WithCancel(context.Background())

	calls := 0
	fn := func() error {
		calls++
		return errors.New("transient")
	}

	// Cancel after 50ms, well before the 5s first retry delay fires.
	go func() {
		time.Sleep(50 * time.Millisecond)
		cancel()
	}()

	start := time.Now()
	err := withRetry(ctx, "test", nil, fn)
	elapsed := time.Since(start)

	if !errors.Is(err, context.Canceled) {
		t.Errorf("expected context.Canceled, got %v", err)
	}
	if elapsed > 2*time.Second {
		t.Errorf("withRetry did not exit promptly after cancellation: took %v", elapsed)
	}
	// fn should be called exactly once (initial attempt); retry sleep is cancelled.
	if calls != 1 {
		t.Errorf("expected 1 fn call, got %d (sleep should interrupt before retry)", calls)
	}
}

// TestWithRetry_ContextAlreadyCancelledBeforeCall verifies that a pre-cancelled
// context causes the first attempt to run (fn is called), and if it fails the
// sleep is immediately interrupted.
func TestWithRetry_ContextAlreadyCancelledBeforeCall(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{5 * time.Second}
	defer func() { trackRetryDelays = origDelays }()

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // cancel immediately

	calls := 0
	start := time.Now()
	err := withRetry(ctx, "test", nil, func() error {
		calls++
		return errors.New("fail")
	})
	elapsed := time.Since(start)

	if !errors.Is(err, context.Canceled) {
		t.Errorf("expected context.Canceled, got %v", err)
	}
	if elapsed > 500*time.Millisecond {
		t.Errorf("already-cancelled context did not short-circuit sleep: took %v", elapsed)
	}
}

// TestWithRetry_OnRetryCallbackFired verifies the onRetry callback is called
// before each retry sleep.
func TestWithRetry_OnRetryCallbackFired(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{1 * time.Millisecond, 1 * time.Millisecond}
	defer func() { trackRetryDelays = origDelays }()

	var retryAttempts []int
	err := withRetry(context.Background(), "test", func(attempt, max int, wait time.Duration, lastErr error) {
		retryAttempts = append(retryAttempts, attempt)
	}, func() error {
		return errors.New("fail")
	})

	if err == nil {
		t.Fatal("expected error")
	}
	if len(retryAttempts) != 2 {
		t.Errorf("expected 2 onRetry calls, got %d: %v", len(retryAttempts), retryAttempts)
	}
	if retryAttempts[0] != 1 || retryAttempts[1] != 2 {
		t.Errorf("unexpected retry attempt numbers: %v", retryAttempts)
	}
}

// ---- Fakes used by processJob / Run tests ---------------------------------

// fakeRunner implements MetadataFetcher. metaFn is called for FetchMetadata;
// audioFn (optional) is called for FetchAudio — defaults to writing "fake audio".
type fakeRunner struct {
	metaFn  func(uri string) ([]byte, error)
	audioFn func(uri, fileID string, w io.Writer) error
}

func (f *fakeRunner) FetchMetadata(_ *credentials.Credentials, uri string) ([]byte, error) {
	return f.metaFn(uri)
}

func (f *fakeRunner) FetchAudio(_ *credentials.Credentials, uri, fileID string, w io.Writer) error {
	if f.audioFn != nil {
		return f.audioFn(uri, fileID, w)
	}
	_, _ = w.Write([]byte("fake audio data"))
	return nil
}

// fakeCreds implements CredentialProvider, always returning a stub token.
type fakeCreds struct{}

func (f *fakeCreds) Get() (*credentials.Credentials, error) {
	return &credentials.Credentials{AccessToken: "test"}, nil
}

// fakeTagger implements AudioTagger. If err is non-nil it is returned on every call.
type fakeTagger struct{ err error }

func (f *fakeTagger) TagTrack(_ string, _ *spotify.Track) error   { return f.err }
func (f *fakeTagger) TagEpisode(_ string, _ *spotify.Episode) error { return f.err }

// newTestWorker builds a Worker backed by a real in-memory queue and a temp storage dir.
func newTestWorker(t *testing.T, runner MetadataFetcher, tgr AudioTagger) (*Worker, *queue.Queue) {
	t.Helper()
	dir := t.TempDir()
	q, err := queue.New(dir + "/q.db")
	if err != nil {
		t.Fatalf("queue.New: %v", err)
	}
	t.Cleanup(func() { q.Close() })
	return &Worker{
		queue:        q,
		runner:       runner,
		creds:        &fakeCreds{},
		store:        storage.New(dir),
		tagger:       tgr,
		progress:     progress.NewStore(),
		pollInterval: 1 * time.Millisecond,
		concurrency:  1,
	}, q
}

// countFilesWithExt walks root and counts files whose name ends with ext.
func countFilesWithExt(t *testing.T, root, ext string) int {
	t.Helper()
	n := 0
	_ = filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		if err == nil && !info.IsDir() && strings.HasSuffix(path, ext) {
			n++
		}
		return nil
	})
	return n
}

// Minimal JSON fixtures --------------------------------------------------

const trackJSON = `{
  "type": "track",
  "uri": "spotify:track:t1",
  "name": "Test Track",
  "duration_ms": 180000,
  "artists": [{"uri":"spotify:artist:a","name":"Test Artist"}],
  "album": {"uri":"spotify:album:a","name":"Test Album"},
  "files": [{"file_id":"fid001","format":"OGG_VORBIS_160"}]
}`

const albumJSON = `{
  "type": "album",
  "uri": "spotify:album:a",
  "name": "Test Album",
  "artists": [{"uri":"spotify:artist:a","name":"Test Artist"}],
  "discs": [{"number":1,"tracks":["spotify:track:t1","spotify:track:t2"]}]
}`

// track2JSON is a second track that belongs to the same album.
const track2JSON = `{
  "type": "track",
  "uri": "spotify:track:t2",
  "name": "Second Track",
  "duration_ms": 240000,
  "artists": [{"uri":"spotify:artist:a","name":"Test Artist"}],
  "album": {"uri":"spotify:album:a","name":"Test Album"},
  "files": [{"file_id":"fid002","format":"OGG_VORBIS_160"}]
}`

const episodeJSON = `{
  "type": "episode",
  "uri": "spotify:episode:e1",
  "name": "Test Episode",
  "show_name": "Test Show",
  "duration_ms": 3600000,
  "audio_files": [{"file_id":"efid001","format":"OGG_VORBIS_96"}]
}`

// ---- processJob / Run integration tests -----------------------------------

// TestRun_SingleTrackHappyPath verifies the end-to-end path for a single track:
// metadata fetched twice (job-level + track-level), audio written to disk, job done.
func TestRun_SingleTrackHappyPath(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = nil
	defer func() { trackRetryDelays = origDelays }()

	runner := &fakeRunner{
		metaFn: func(_ string) ([]byte, error) { return []byte(trackJSON), nil },
	}
	w, q := newTestWorker(t, runner, &fakeTagger{})
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:track:t1")

	if err := w.Run(context.Background(), true); err != nil {
		t.Fatalf("Run: %v", err)
	}

	jobs, _ := q.List()
	if jobs[0].Status != queue.StatusDone {
		t.Errorf("job status = %s, want done", jobs[0].Status)
	}
	if countFilesWithExt(t, w.store.BaseDir, ".ogg") != 1 {
		t.Error("expected 1 .ogg file on disk")
	}
}

// TestRun_EpisodeHappyPath verifies episodes are fetched and tagged correctly.
func TestRun_EpisodeHappyPath(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = nil
	defer func() { trackRetryDelays = origDelays }()

	runner := &fakeRunner{
		metaFn: func(_ string) ([]byte, error) { return []byte(episodeJSON), nil },
	}
	w, q := newTestWorker(t, runner, &fakeTagger{})
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:episode:e1")

	if err := w.Run(context.Background(), true); err != nil {
		t.Fatalf("Run: %v", err)
	}

	jobs, _ := q.List()
	if jobs[0].Status != queue.StatusDone {
		t.Errorf("job status = %s, want done", jobs[0].Status)
	}
	if countFilesWithExt(t, w.store.BaseDir, ".ogg") != 1 {
		t.Error("expected 1 .ogg file for episode")
	}
}

// TestRun_MetadataFetchFails verifies that a metadata error causes processJob to
// return an error (which the Run loop records as a job failure).
func TestRun_MetadataFetchFails(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = nil
	defer func() { trackRetryDelays = origDelays }()

	runner := &fakeRunner{
		metaFn: func(_ string) ([]byte, error) {
			return nil, errors.New("network down")
		},
	}
	w, q := newTestWorker(t, runner, &fakeTagger{})
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:track:t1")

	// Grab the job and call processJob directly so we can assert on the returned
	// error without having to exhaust the queue's own retry schedule.
	job, err := q.Next()
	if err != nil || job == nil {
		t.Fatal("expected a pending job")
	}

	procErr := w.processJob(context.Background(), job)
	if procErr == nil {
		t.Fatal("expected processJob to return an error")
	}
	if !strings.Contains(procErr.Error(), "network down") {
		t.Errorf("error = %q, want mention of 'network down'", procErr.Error())
	}
}

// TestRun_AudioFetchFails_TrackSkipped verifies that exhausted audio-fetch retries
// for a single track do NOT fail the overall job — the track is skipped but the
// job completes.
func TestRun_AudioFetchFails_TrackSkipped(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = nil // no per-track retries
	defer func() { trackRetryDelays = origDelays }()

	runner := &fakeRunner{
		metaFn: func(_ string) ([]byte, error) { return []byte(trackJSON), nil },
		audioFn: func(_, _ string, _ io.Writer) error {
			return errors.New("server error")
		},
	}
	w, q := newTestWorker(t, runner, &fakeTagger{})
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:track:t1")

	if err := w.Run(context.Background(), true); err != nil {
		t.Fatalf("Run: %v", err)
	}

	jobs, _ := q.List()
	if jobs[0].Status != queue.StatusDone {
		t.Errorf("job status = %s, want done (track skipped, not failed)", jobs[0].Status)
	}
	if countFilesWithExt(t, w.store.BaseDir, ".ogg") != 0 {
		t.Error("expected no .ogg file (audio fetch failed)")
	}
}

// TestRun_Album_WritesPlaylistFile verifies that fetching an album enqueues all
// its tracks and produces an M3U8 playlist file alongside the audio.
func TestRun_Album_WritesPlaylistFile(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = nil
	defer func() { trackRetryDelays = origDelays }()

	runner := &fakeRunner{
		metaFn: func(uri string) ([]byte, error) {
			if strings.Contains(uri, "album") {
				return []byte(albumJSON), nil
			}
			if uri == "spotify:track:t2" {
				return []byte(track2JSON), nil
			}
			return []byte(trackJSON), nil
		},
	}
	w, q := newTestWorker(t, runner, &fakeTagger{})
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:album:a")

	if err := w.Run(context.Background(), true); err != nil {
		t.Fatalf("Run: %v", err)
	}

	jobs, _ := q.List()
	if jobs[0].Status != queue.StatusDone {
		t.Errorf("job status = %s, want done", jobs[0].Status)
	}
	if countFilesWithExt(t, w.store.BaseDir, ".ogg") != 2 {
		t.Errorf("expected 2 .ogg files (one per track), got %d", countFilesWithExt(t, w.store.BaseDir, ".ogg"))
	}
	if countFilesWithExt(t, w.store.BaseDir, ".m3u8") != 1 {
		t.Error("expected 1 .m3u8 playlist file")
	}
}

// TestRun_ContextCancelled_BeforeProcessing verifies that a pre-cancelled context
// causes Run to return immediately without processing any job.
func TestRun_ContextCancelled_BeforeProcessing(t *testing.T) {
	runner := &fakeRunner{
		metaFn: func(_ string) ([]byte, error) {
			t.Error("metaFn should not be called when context is pre-cancelled")
			return nil, nil
		},
	}
	w, q := newTestWorker(t, runner, &fakeTagger{})
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:track:t1")

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // cancel before Run

	err := w.Run(ctx, true)
	if !errors.Is(err, context.Canceled) {
		t.Errorf("expected context.Canceled, got %v", err)
	}

	// Job should still be in pending state — never dequeued.
	jobs, _ := q.List()
	if jobs[0].Status != queue.StatusPending {
		t.Errorf("job status = %s, want pending (context cancelled before processing)", jobs[0].Status)
	}
}

// TestRun_AlreadyFetched_SkipsAudio verifies that a track whose output file
// already exists is reported as AlreadyPresent without calling FetchAudio again.
func TestRun_AlreadyFetched_SkipsAudio(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = nil
	defer func() { trackRetryDelays = origDelays }()

	audioCalls := 0
	runner := &fakeRunner{
		metaFn: func(_ string) ([]byte, error) { return []byte(trackJSON), nil },
		audioFn: func(_, _ string, _ io.Writer) error {
			audioCalls++
			return nil
		},
	}
	w, q := newTestWorker(t, runner, &fakeTagger{})

	// First run: file is downloaded (audioCalls == 1).
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:track:t1")
	if err := w.Run(context.Background(), true); err != nil {
		t.Fatalf("first Run: %v", err)
	}
	if audioCalls != 1 {
		t.Fatalf("expected 1 audio call on first run, got %d", audioCalls)
	}

	// Second run: file already exists — audio fetch must not be called.
	_, _ = q.Enqueue(queue.EnqueueOptions{}, "spotify:track:t1")
	if err := w.Run(context.Background(), true); err != nil {
		t.Fatalf("second Run: %v", err)
	}
	if audioCalls != 1 {
		t.Errorf("FetchAudio called %d times after second run, want 1 (file already present)", audioCalls)
	}

	jobs, _ := q.List()
	for _, j := range jobs {
		if j.Status != queue.StatusDone {
			t.Errorf("job id=%d status=%s, want done", j.ID, j.Status)
		}
	}
}

// jsonUnmarshal is intentionally omitted — use spotify.ParseMetadata directly.
