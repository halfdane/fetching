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
| `worker` | ✅ covered | Happy path, audio-fail skip, metadata-fail, album M3U8, ctx cancel, skip-already-present | Interface mocks |
| `tagger` | ✅ covered | TagTrack/TagEpisode happy + failure, metadata args, buildArgs | TestMain helper-process |
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

## Phase 3 — Worker (interface mocks) ✅ done

`internal/worker/deps.go` defines three interfaces:
- `MetadataFetcher` — `FetchMetadata` + `FetchAudio` (satisfied by `*cli.Runner`)
- `CredentialProvider` — `Get()` (satisfied by `*credentials.Store`)
- `AudioTagger` — `TagTrack` + `TagEpisode` (satisfied by `*tagger.Tagger`)

`Worker` struct fields and `New()` use these interfaces, eliminating the `cli` and `tagger` package imports.

`internal/worker/worker_test.go` — 12 tests (7 new + 5 existing `withRetry`):
- `TestRun_SingleTrackHappyPath` — track → .ogg on disk, job `done`
- `TestRun_EpisodeHappyPath` — episode → .ogg, job `done`
- `TestRun_MetadataFetchFails` — top-level error propagated from `processJob`
- `TestRun_AudioFetchFails_TrackSkipped` — audio error → track skipped, job still `done`
- `TestRun_Album_WritesPlaylistFile` — 2 tracks + .m3u8 produced
- `TestRun_ContextCancelled_BeforeProcessing` — pre-cancelled ctx returns immediately
- `TestRun_AlreadyFetched_SkipsAudio` — second run does not call `FetchAudio` again

**Bug fixes found by tests:**
- `fetchTrack` and `fetchEpisode` now remove the partial/empty output file when audio download fails, so a later `os.Stat` skip-check is not fooled into thinking the track was already downloaded.

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

## Phase 5 — Tagger (subprocess mock) ✅ done

`internal/tagger/tagger.go` gains an unexported `cmdFunc func(string, ...string) *exec.Cmd`
field (nil = use `exec.Command`). No public API change; `New()` is unchanged.

`internal/tagger/tagger_test.go` uses the **TestMain helper-process** pattern:
the test binary re-invokes itself with `GO_WANT_HELPER_PROCESS=1` as a minimal
stub that copies the input file to the output path (simulating ffmpeg's
copy-then-rename strategy). `FAKE_FFMPEG_FAIL=1` forces failure;
`FAKE_FFMPEG_ARGS_FILE=<path>` captures the argument list.

8 tests:
- `TestTagTrack_HappyPath` / `TestTagEpisode_HappyPath` — file intact after tag
- `TestTagTrack_FfmpegFailure` / `TestTagEpisode_FfmpegFailure` — error propagated, temp file cleaned up
- `TestTagTrack_PassesMetadataArgs` / `TestTagEpisode_PassesMetadataArgs` — title/album/artist in ffmpeg args
- `TestBuildArgs_OGGNoCoverDoesNotUsePicStream` — OGG branch avoids `-map`/`-c:v`
- `TestBuildArgs_MP3HasId3v2` — MP3 branch includes `-id3v2_version 3`

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
  queue/         queue_test.go          ✅ done (Phase 2)
  progress/      store_test.go          ✅ done
  worker/        worker_test.go         ✅ done (Phase 3)
               deps.go                ✅ done (Phase 3 interfaces)
  web/           handler_test.go        ✅ done (Phase 4)
  tagger/        tagger_test.go         ✅ done (Phase 5)
```
