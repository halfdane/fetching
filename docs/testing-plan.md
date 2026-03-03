# Testing Plan

## Current state

| Package | Status | Coverage | Notes |
|---------|--------|----------|-------|
| `spotify` | ✅ covered | Types, parsing, helpers | Pure functions, no runtime deps |
| `playlist` | ✅ covered | M3U8 generation | Writes to `t.TempDir()` |
| `storage` | ✅ covered | Path construction, sanitization | Pure path logic, no disk writes |
| `cover` | ✅ covered | Drawing, composition, HTTP routing | Uses `httptest.Server` for network calls |
| `queue` | ✅ covered | Enqueue, Next, Complete, Fail, List, Retry, RecoverStuckJobs | SQLite `:memory:` DB |
| `progress` | ✅ covered | Snapshot, log, concurrent access | — |
| `web` | ✅ covered | Enqueue, retry, list, SSE stream, JSON/HTML | `httptest` + mock queue |
| `worker` | ❌ not covered | — | Needs mocks for all external I/O |
| `tagger` | ❌ not covered | — | Needs mock for ffmpeg subprocess |
| `cli` | ❌ not covered | — | Wraps external binary |
| `credentials` | ❌ not covered | — | Filesystem I/O |

---

## Phase 1 — Low-hanging fruit (done)

The packages above with ✅ (first four) are pure functions or local file I/O; tests need no mocks and run offline.

---

## Phase 2 — Queue (SQLite in-memory) ✅ done

`internal/queue/queue_test.go` — 21 tests covering:
- `Enqueue` creates jobs with `pending` status; `FallbackQuality` flag stored; multi-URI batch
- `Next` returns `nil` on empty queue; marks claimed job `running`, sets `StartedAt` and `goqite_msg_id`; second call returns nil while job is running
- `Complete` marks job `done`, clears `goqite_msg_id`
- `Fail` with retries remaining → re-enqueues, increments `retry_count` (overrides `retryDelays` to `[1ms]`)
- `Fail` after max retries → marks job `failed` with error text (overrides `retryDelays` to `[]`)
- `List` returns all jobs newest-first (`ORDER BY created_at DESC, id DESC`)
- `Retry` deletes terminal jobs and creates a fresh pending one; no-op when nothing to delete; does not touch running jobs
- `RecoverStuckJobs` resets running jobs after visibility timeout; discards stale goqite messages

**Key implementation notes:**
- `maxRetries()` is a function (not a `var`) so test overrides of `retryDelays` take effect at call-time
- `finishJob` clears `goqite_msg_id` in the terminal `UPDATE`
- `List` uses `ORDER BY created_at DESC, id DESC` to be deterministic when rows share a second-level timestamp

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
- Audio fetch fails → `withRetry` retries; after all retries, job completes (track skipped)
- Metadata fetch fails → job marked `failed` and eventually retried at queue level
- Album with mixed success/failure → partial results → M3U8 still written for successful tracks
- Context cancellation mid-loop → job fails fast

**File:** `internal/worker/worker_test.go`

---

## Phase 4 — Web handler ✅ done

`internal/web/handler_test.go` covers the HTTP layer via `httptest.NewRecorder` and a `fakeQueue`
stub implementing the `Queuer` interface:

- `GET /` → 200, renders index template with job list
- `POST /enqueue` with valid URI → 303 redirect, job added
- `POST /enqueue` with `Accept: application/json` → 200 JSON
- `POST /retry` → deletes terminal jobs, creates fresh pending job
- `GET /jobs` → 200, SSR job-list partial (HTMX target)
- `GET /api/stream` → SSE endpoint; sends `snapshot` + `log` events

HTML templates live in `internal/web/templates/*.html` (real files, loaded via `//go:embed`):
- `index.html` — full page with CSS + JS (SSE client, retry buttons, log console)
- `jobs.html` — SSR partial (`{{define "jobs"}}`)

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
