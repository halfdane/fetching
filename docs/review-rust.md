# Code Review — Rust Implementation (`main-old` branch)

> Reviewed against: `67480f9` (v1.0.6)
> Scope: entire Cargo workspace — `main.rs`, `core/`, `server/`

---

## Summary

The Rust implementation is architecturally richer than the Go version: it embeds librespot
directly, delivers a full SPA frontend, and has a more sophisticated data model (normalised
SQLite schema, broadcast-channel SSE, `Arc`-shared state). The trade-off is significantly
higher build complexity, a larger dependency surface, and a `db.rs` that has grown to
1 061 LOC. Tests are concentrated in `core/` and are well-structured, but the `runner.rs`
finalization path has a known rough edge, and `server/` has no tests at all.

---

## Code Quality

### 🟢 `DownloadCoordinator` is a clean central abstraction

The coordinator owns the queue, the database reference, the progress broadcast channel, and
the background worker task. The `JobRunner` trait cleanly separates the "what to do for one
track" concern from the "when and how to schedule it" concern. Both are testable
independently.

### 🟢 `spawn_blocking` for librespot I/O is the right call

librespot's audio decryption is synchronous and CPU/IO-heavy. Running it inside
`spawn_blocking` keeps the tokio executor free for HTTP handling and channel operations.
The single-permit `Semaphore` in `worker_loop` makes the one-download-at-a-time contract
explicit and easy to change (increase permits for parallelism).

### 🟢 `anyhow` and structured tracing are used consistently

`anyhow::Result` propagates errors ergonomically, and `tracing` with structured key-value
pairs provides useful observability. Field names are consistent (`task_id`, `path`, etc.).

### 🟡 `db.rs` is 1 061 LOC — the biggest single file in the project

The file contains the schema, migration, all CRUD helpers, row types, and status
serialisation. This is functional but makes it hard to navigate. Suggested split:

```
core/src/db/
    mod.rs        — public re-exports + Database struct
    schema.rs     — create/migrate
    collections.rs
    tasks.rs
    recovery.rs
```

### 🟡 `runner.rs` `find_track_file` is a best-effort heuristic

`find_track_file` (lines 300–360 of runner.rs) scans the output directory with a wildcard
glob because the function receives only the `track_uri`, not the resolved `Track` struct.
This means:

1. It returns the **first** matched audio file in the album directory — it does not verify
   that the file actually belongs to `track_uri`.
2. For a directory with 30 tracks, the loop exits on the first found file every time,
   giving all 30 tracks the same path.

This causes the M3U8 playlist to contain duplicated entries when a collection is finalised.

**Suggested fix:** Pass the resolved `Track` struct (or its `title` + `number`) through the
`QueueEntry` or write the final path back to the database when a track completes, so
`maybe_finalise` can do an exact lookup.

### 🟡 `write_cover_from_track` is a no-op in finalisation

The function body only checks whether `cover.jpg` already exists and logs a debug line:
in the 
```rust
fn write_cover_from_track(&self, _collection: &TrackCollection, dir: &Path) {
    let cover_path = dir.join("cover.jpg");
    if cover_path.exists() {
        debug!(path = %cover_path.display(), "Cover already exists");
    }
}
```

Cover writing in finalisation was not re-implemented after the redesign. Albums/playlists
with no downloaded tracks will silently skip cover generation.

---

## Architecture

### 🟢 Workspace split is justified

The three-crate workspace (`fetching`, `fetching-core`, `server`) exists because
`librespot-core` has heavy build-time dependencies (OpenSSL, avahi, cmake). Keeping these in
`core` lets the `server` crate compile without them conceptually (even though the binary
wires both together). It also means the public API surface of each crate is clearly
delimited.

### 🟡 No retry loop for individual tracks

`DownloadRunner::run` returns an error on network failure and the coordinator marks the task
`Failed`. There is no per-track retry logic comparable to the Go implementation's
`withRetry`. A failed track requires a full re-enqueue from the API.

The Go implementation's three-tier retry (channel-level) + outer candidate loop provides
significantly more resilience for transient network errors.

### 🟡 `AppState::collection_metadata` used only in `POST /api/queue`

The `Arc<dyn SpotifyCollectionMetadata>` in `AppState` is used only in the queue handler to
resolve a URI before enqueuing. It is also stored redundantly in `main.rs`'s
`build_apis(...).collection_metadata`. Passing it directly to the handler (rather than via
shared state) would simplify the ownership graph.

### 🟡 Batch mode has coarser progress granularity

In batch mode the coordinator only emits `Running` and `Done`/`Failed` updates. The
per-stage breakdown (`Fetching cover art…`, `Downloading audio…`, `Writing tags…`) emitted
by `runner.rs` is only visible via the SSE stream — there is no equivalent print to stdout,
making batch mode harder to diagnose when something takes a long time.

### 🔴 SPA frontend build is a required step that can silently fail

The Nix package derives the SPA from a separate `buildNpmPackage` step. If the frontend
build fails (or `npmDepsHash` is stale), the release binary will embed an empty
`frontend/build` directory and serve `404` for every page. There is no compile-time
assertion that the embedded asset set is non-empty.

Building in debug mode (`cargo build`) does not embed any assets at all (the `ServeDir`
fallback reads from a local `frontend/build/` path that must exist). This means a fresh
checkout that runs `cargo run` without first building the frontend yields a broken UI with
no error.

**Suggested fix:** Add a `build.rs` assertion or a feature flag that causes the compilation
to fail with a clear message when `frontend/build/index.html` is absent.

---

## Testing

### 🟢 80 test cases collocated with source (Rust idiom)

Tests live in `#[cfg(test)]` modules directly in the source files. The coordinator tests
(`coordinator.rs` lines 400–673) are comprehensive: they cover order preservation, Running→
Done sequencing, Running→Failed sequencing, multi-track completion, single-concurrency, and
SSE wire format. All use in-memory stubs, so they run fast and without filesystem I/O.

### 🟢 In-memory database for fast unit tests

`Database::open_in_memory()` is used in db tests, avoiding temp files and letting tests
run in parallel.

### 🟡 `server/` has no tests

All HTTP handler logic in `server/src/server.rs` is untested. The `queue_url`, `get_
collections`, `get_collection_tracks`, and `events` handlers are exercised only by manual
testing.

**Suggested fix:** Add `axum::test::TestClient` (or `reqwest` + `axum::serve`) tests for
at least the happy and error paths of `queue_url` and `get_collections`.

### 🟡 `runner.rs` has no unit tests

The production `DownloadRunner::run` pipeline (the most complex function in the project) has
zero tests. Only `coordinator.rs` tests exist, which use stub `OkRunner` / `FailRunner`
implementations. End-to-end correctness of the metadata→audio→tag→playlist pipeline depends
entirely on manual smoke testing.

### 🟡 Test stubs use `unimplemented!()`

Most stub trait methods panic with `unimplemented!()`. If a future refactor adds a code
path that calls one of these methods, the test will panic rather than fail with a useful
assertion. Consider returning `Err(anyhow::anyhow!("stub"))` or adding `todo!()` with a
message.

---

## Security

### 🟢 No secrets in code

Credentials are read from a JSON file via `create_session` and never appear in logs or
database columns.

### 🟡 `Mutex<Connection>` is a coarse lock

`db.rs` wraps a single `rusqlite::Connection` in a `std::sync::Mutex`. Every DB operation
blocks the entire mutex: concurrent HTTP handlers (`get_collections`, `get_tracks_for_
collection`) will serialize on this lock. For the expected load (a handful of web sessions)
this is fine, but WAL mode is enabled, so a connection pool (`r2d2` + `r2d2-sqlite`) could
reduce handler latency without schema changes.

### 🟢 No SQL injection surface

All queries use `rusqlite`'s `params![]` macro (positional `?` binding). No string
interpolation into SQL is used.

---

## Performance

### 🟢 `moka` cache for cover art

`CachedCoverProvider` wraps a `moka::future::Cache<String, Vec<u8>>` keyed on cover ID.
Albums with shared covers (re-releases, box sets) fetch cover art only once per server
lifetime. This is a meaningful optimisation that the Go implementation lacks.

### 🟢 SSE via broadcast channel

`tokio::sync::broadcast` is the right primitive: all SSE subscribers share one sender, and
lagged receivers are automatically skipped (`RecvError::Lagged`). The Go implementation
requires a per-subscriber goroutine and a separate fan-out loop.

### 🟡 `db.insert_collection_with_tracks` is not wrapped in a transaction

The function inserts one row into `collections` and then N rows into `tracks` in a loop,
each as a separate statement. On a slow disk or with a large playlist (1 000+ tracks), this
is O(N) round-trips. Wrapping the entire insert in `BEGIN … COMMIT` would reduce write
latency by ~100×.

---

## Documentation

### 🟢 Doc comments on public API surface

`DownloadCoordinator`, `JobRunner`, `DownloadRunner`, `Database`, and the major public
methods all have /// doc comments explaining their purpose and usage.

### 🟡 `core/src/lib.rs` is a bare re-export list

There is no crate-level doc comment explaining what `fetching-core` does, what the
entry points are, or which types callers should start with. A short module-level comment
would help newcomers navigating the workspace.

---

## Checklist

| Category       | Status |
|----------------|--------|
| Code style     | ✅ Idiomatic Rust, consistent formatting |
| Error handling | ✅ `anyhow` throughout; non-fatal paths log warnings |
| Naming         | ✅ Descriptive, snake_case, no stutter |
| Test coverage  | 🟡 `core/` well tested; `server/` and `runner.rs` untested |
| Security       | ✅ No obvious vulnerabilities; coarse DB lock acceptable |
| Performance    | 🟡 Missing transaction wrapping for bulk inserts |
| Architecture   | 🟡 No per-track retry; `find_track_file` heuristic is unreliable |
| Documentation  | 🟡 Good on public types; crate-level doc missing |
