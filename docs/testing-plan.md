# Testing Plan

## Current state

| Package | Status | Coverage | Notes |
|---------|--------|----------|-------|
| `spotify` | ✅ covered | Types, parsing, helpers | Pure functions, no runtime deps |
| `playlist` | ✅ covered | M3U8 generation | Writes to `t.TempDir()` |
| `storage` | ✅ covered | Path construction, sanitization | Pure path logic, no disk writes |
| `cover` | ✅ covered | Drawing, composition, HTTP routing | Uses `httptest.Server` for network calls |
| `queue` | ❌ not covered | — | Needs SQLite in-memory DB |
| `worker` | ❌ not covered | — | Needs mocks for all external I/O |
| `tagger` | ❌ not covered | — | Needs mock for ffmpeg subprocess |
| `cli` | ❌ not covered | — | Wraps external binary |
| `credentials` | ❌ not covered | — | Filesystem I/O |
| `web` | ❌ not covered | — | HTTP handler, needs mock queue |

---

## Phase 1 — Low-hanging fruit (done)

The packages above with ✅ are already tested. They share a property: **all logic is pure functions or local file I/O**, so tests need no mocks and run offline.

---

## Phase 2 — Queue (SQLite in-memory)

The `queue` package is fully self-contained once given a DB path. Go's `database/sql` accepts
`":memory:"` for SQLite, so no mocks are needed — just an in-process DB.

**Approach:** pass `":memory:"` to `queue.New()`

```go
func TestEnqueueAndNext(t *testing.T) {
    q, _ := queue.New(":memory:")
    defer q.Close()

    jobs, err := q.Enqueue("spotify:track:abc")
    // ...
    job, err := q.Next()
    // ...
}
```

**Test cases to write:**
- `Enqueue` → creates job with `pending` status
- `Next` → returns job, marks it `running`; returns nil on empty queue
- `Complete` → marks job `done`, removes from goqite
- `Fail` with retries remaining → re-enqueues with delay, increments `retry_count`
- `Fail` after max retries → marks job `failed` permanently
- `List` → returns all jobs newest-first
- Crash recovery: goqite visibility timeout re-surfaces unacked message

**File:** `internal/queue/queue_test.go`

---

## Phase 3 — Worker (interface mocks)

The worker depends on four external things. Introduce interfaces so tests can inject fakes.

### Define interfaces

```go
// internal/worker/deps.go

type MetadataRunner interface {
    FetchMetadata(creds *credentials.Credentials, uri string) ([]byte, error)
    FetchAudio(creds *credentials.Credentials, uri, fileID string, w io.Writer) error
}

type CredentialStore interface {
    Get() (*credentials.Credentials, error)
}

type AudioTagger interface {
    TagTrack(path string, track *spotify.Track) error
    TagEpisode(path string, ep *spotify.Episode) error
}
```

The `storage.Storage` and `queue.Queue` can be used directly in tests (using
`t.TempDir()` + `:memory:` DB) rather than mocked.

### Fake implementations (in `_test.go` files)

```go
type fakeRunner struct {
    trackJSON  []byte
    audioError error
}

func (f *fakeRunner) FetchMetadata(_ *credentials.Credentials, _ string) ([]byte, error) {
    return f.trackJSON, nil
}

func (f *fakeRunner) FetchAudio(_ *credentials.Credentials, _, _ string, w io.Writer) error {
    if f.audioError != nil {
        return f.audioError
    }
    _, _ = w.Write([]byte("fake audio data"))
    return nil
}
```

**Test cases to write:**
- Full happy path: single track → file on disk, job marked `done`
- Audio download fails → `withRetry` retries; after all retries, job completes (track skipped)
- Metadata fetch fails → job marked `failed` and eventually retried at queue level
- Album with mixed success/failure → partial results → M3U8 still written for successful tracks
- Context cancellation mid-loop → job fails fast

**File:** `internal/worker/worker_test.go`

---

## Phase 4 — Web handler

The web handler only needs a mock `Queuer` interface.

### Define interface

```go
// internal/web/handler.go (or deps.go)

type Queuer interface {
    Enqueue(uris ...string) ([]*queue.Job, error)
    List() ([]*queue.Job, error)
}
```

### Testing approach

Use Go's `net/http/httptest` package:

```go
func TestHandlerEnqueue(t *testing.T) {
    fakeQ := &fakeQueue{}
    h := web.NewHandler(fakeQ)
    srv := httptest.NewServer(h)
    defer srv.Close()

    resp, _ := http.PostForm(srv.URL+"/enqueue", url.Values{"uri": {"spotify:track:abc"}})
    if resp.StatusCode != http.StatusSeeOther {
        t.Errorf("expected redirect, got %d", resp.StatusCode)
    }
    if len(fakeQ.enqueued) != 1 || fakeQ.enqueued[0] != "spotify:track:abc" {
        t.Errorf("unexpected enqueued: %v", fakeQ.enqueued)
    }
}
```

**Test cases to write:**
- `GET /` → 200, renders job list
- `POST /enqueue` with valid URI → 303 redirect, job added
- `POST /enqueue` with empty URI → 400
- `GET /jobs` (HTMX partial) → 200, HTML fragment containing job statuses

**File:** `internal/web/handler_test.go`

---

## Phase 5 — Tagger (subprocess mock)

The tagger shells out to `ffmpeg`. Options:

1. **Wrap the call** behind an interface `type FFmpegRunner interface { Run(args ...string) error }` and inject a fake in tests.
2. **Integration test only** behind a build tag: `//go:build integration` — requires ffmpeg on PATH.

Recommended: do both. Unit tests use the fake; the integration test is gated and only runs in CI where ffmpeg is available.

**File:** `internal/tagger/tagger_test.go`

---

## Phase 6 — Integration / end-to-end

Gated behind `//go:build integration`. Requires fetching-cli and ffmpeg on PATH.

A single test that:
1. Starts a real `Queue` with `:memory:`
2. Enqueues a known-stable public track
3. Runs `Worker.Run(ctx, oneShot=true)`
4. Asserts the file exists on disk and has valid audio tags

```bash
go test -tags integration ./...
```

---

## Tooling conventions

- All unit tests must **run offline** with `go test ./...` (no real Spotify/network calls)
- Use `t.TempDir()` for any disk output — cleaned up automatically
- Use `t.Parallel()` in pure-function tests for faster CI
- Target: `go test -race ./...` passes cleanly
- CI (GitHub Actions): runs `go test ./...` on every push; integration tests run weekly on schedule

---

## File layout

```
internal/
  spotify/       spotify_test.go        ✅ done
  playlist/      playlist_test.go       ✅ done
  storage/       storage_test.go        ✅ done
  cover/         cover_test.go          ✅ done
  queue/         queue_test.go          Phase 2
  worker/        worker_test.go         Phase 3
               deps.go                Phase 3 (interfaces)
  web/           handler_test.go        Phase 4
  tagger/        tagger_test.go         Phase 5
```
