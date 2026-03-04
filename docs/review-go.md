# Code Review — Go Implementation (`main` branch)

> Reviewed against: `a4d87cb` (v0.1.5), updated after IPC/auth refactor
> Scope: all Go source under `cmd/` and `internal/`

---

## Summary

The Go rewrite is lean, well-structured, and idiomatic. It deliberately offloads the one
unavoidable native dependency (librespot) to a separate binary (`fetching-cli`) rather than
linking it in, which keeps the Go module almost dependency-free. Test coverage is notably
thorough (149 test functions across 10 test files). The main weakness is that the IPC
boundary to `fetching-cli` is invisible to the test suite and is brittle at deployment time.

---

## Code Quality

### 🟢 Package layout

The `internal/` tree maps cleanly to domain concepts — `queue`, `worker`, `progress`,
`storage`, `tagger`, `spotify`, `web`. Each package is small enough to fit in one file (or
at most two). The naming avoids stutter and follows Go conventions throughout.

### 🟢 Idiomatic patterns

- **Early-return + guard clauses** are used consistently across worker.go and handler.go.
- **`context.Context` propagation** through the worker loop correctly cancels in-flight
  retries when the server shuts down.
- **Interface injection** (`MetadataFetcher`, `AudioTagger`) makes every dependency in
  `Worker` swappable in tests without build tags. Credential management was fully
  delegated to `fetching-cli`, eliminating the former `CredentialProvider` interface.
- **Structured logging** (`log/slog`) is used instead of raw `fmt.Println`.

### 🟡 `worker.go` is ~640 LOC — consider splitting

`processJob` handles metadata resolution, per-track fetching, playlist generation, and
cover art, all in one file. The `generate*Assets` family of methods could live in a
`worker_assets.go` file to reduce cognitive load when reading the main processing path.

### 🟡 Poll-based idle loop

`Worker.Run` polls `queue.Next()` every `pollInterval` (2 s) when the queue is empty.
A channel-based wake-up (e.g. closing/signalling a `chan struct{} ` from `Enqueue`) would
eliminate the latency and remove the `time.Sleep` entirely. This only matters for
interactive `serve` mode — `batch` mode finishes as soon as the queue drains.

### 🟢 `withRetry` is clean and well-tested

The closure-based design with an `onRetry` callback keeps side-effects (progress updates)
out of the retry engine itself. Five dedicated test cases cover the happy path, all-fail,
context-cancel-during-sleep, pre-cancelled context, and the callback firing sequence — this
is exemplary.

---

## Architecture

### 🔴 CRITICAL — `fetching-cli` subprocess is an implicit runtime dependency

`cli/runner.go` shells out to `fetching-cli` for every metadata fetch, audio download,
and now also for authentication (`EnsureAuth`). The binary must be on `PATH` at runtime.

Since the auth refactor, `EnsureAuth()` is called at startup before any job processing,
so a missing binary *will* cause an immediate startup failure. However, the error comes
from `exec.Command` and is not particularly descriptive.

**Suggested mitigation:** Add an explicit `exec.LookPath` check in `setupDeps` for a
clearer error message:

```go
if _, err := exec.LookPath(runner.Binary); err != nil {
    return nil, nil, nil, nil, fmt.Errorf("%q not found on PATH — see README: %w", runner.Binary, err)
}
```

### 🟡 Progress store is purely in-memory

`progress.Store` holds all live state in a `map[int64]*CollectionView`. A server restart
wipes it completely. The web UI will show an empty list after restart even though the queue
database has pending/completed jobs.

The Rust implementation avoids this by reading state from SQLite on the API calls. A simple
fix would be to rebuild the in-memory snapshot from the `jobs` table on startup (analogous
to `RecoverStuckJobs`).

### 🟢 Queue persistence and crash recovery are handled correctly

`queue.RecoverStuckJobs` resets any `running` jobs back to `pending` before the worker
starts, with a cleanup of the goqite visibility lock. The additive column migration approach
(`ALTER TABLE ... ADD COLUMN` + `isDuplicateColumnErr` guard) is safe for production
upgrades.

### 🟢 goqite abstracts away the SQS visibility timeout pattern

Using goqite means a worker crash (without `Complete`/`Fail`) will automatically re-expose
the goqite message after `jobTimeout` (15 min). This is a good safety net even without
explicit crash-recovery logic.

---

## Testing

### 🟢 Strong coverage of the core processing path

`worker_test.go` (474 LOC) exercises `processJob` end-to-end against a real SQLite queue
and a temp filesystem, using stub implementations of `MetadataFetcher` and `AudioTagger`.
Table-driven tests check album/playlist/episode processing, fallback-quality, already-
fetched idempotency, and partial failure scenarios.

### 🟢 Handler tests use `httptest` correctly

`handler_test.go` builds a full `Handler` with real queue and progress store, sends real
HTTP requests, and asserts on JSON responses. SSE streaming is tested with a timeout.

### 🟡 `generate*Assets` integration gap

`generateAlbumAssets`, `generatePlaylistAssets`, and `generateShowAssets` are exercised
indirectly through `processJob` tests, but there are no dedicated unit tests for the
playlist/cover writing logic with controlled inputs. A failure in `playlist.WriteM3U8` would
only surface as a logged warning, making it easy to miss.

### 🟡 `main.go` wiring is untested

`setupDeps`, `runBatch`, and `runServe` are entirely integration-tested via "go build" only.
A smoke test that calls `runBatch` with a pre-seeded queue and a fake fetching-cli would
catch wiring regressions.

### 🟢 Test helpers follow Go conventions

`newTestWorker` uses `t.TempDir()`, `t.Cleanup(q.Close)`, and a minimal `trackRetryDelays`
override pattern — all clean and well-understood.

---

## Security

### 🟢 No secrets in code

Credentials are fully managed by `fetching-cli` and stored at
`~/.config/fetching-cli/credentials.json`. The Go server never reads, stores, or transmits
OAuth tokens — it simply calls `EnsureAuth()` at startup and trusts the CLI to handle
refresh transparently on subsequent calls.

### 🟢 Graceful shutdown via signal context

`signal.NotifyContext` with `SIGINT`/`SIGTERM` propagates cancellation cleanly to the worker
and the HTTP server.

---

## Performance

### 🟢 Concurrency is configurable

The `--concurrency` flag controls `Worker.concurrency`. The current implementation processes
jobs sequentially when `concurrency == 1`, but the field suggests future parallel support.
(Note: the current `Run` loop is single-threaded — it does not launch goroutines per job.
The concurrency field is referenced but the parallel scheduling logic is not yet present.)

### 🟡 Concurrency field is unused beyond a guard

`Worker.concurrency` is stored and bounds-checked in `New`, but `Run` always processes
one job at a time regardless of the value. This is either intentional with a TODO or a
gap left from a refactor.

---

## Documentation

### 🟢 Package-level comments are present

All packages have a one-line comment explaining their purpose.

### 🟡 `usage` string in `main.go` mentions `--fallback-quality` for `serve`

The `serve` flags section in the usage string omits `--fallback-quality`, but `runBatch`
documents it. Minor, but the flag does exist in the web UI, so the help text is slightly
misleading.

---

## Checklist

| Category       | Status |
|----------------|--------|
| Code style     | ✅ Consistent, idiomatic |
| Error handling | ✅ Errors propagated to callers; warnings logged for non-fatal |
| Naming         | ✅ Clear, no stutter |
| Test coverage  | ✅ Good; gap in `generate*Assets` and `main.go` wiring |
| Security       | ✅ No obvious vulnerabilities |
| Performance    | 🟡 Poll loop; concurrency field unused |
| Architecture   | � Runtime binary dependency — fails at startup but error could be clearer |
| Documentation  | 🟡 Minor usage-string gap |
