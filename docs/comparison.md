# Implementation Comparison: Go vs Rust

> Go implementation: `main` branch (v0.1.5, commit `a4d87cb`)
> Rust implementation: `main-old` branch (v1.0.6, commit `67480f9`)
>
> Individual reviews: [review-go.md](review-go.md) · [review-rust.md](review-rust.md)

---

## At a Glance

| Dimension                   | Go (`main`)                   | Rust (`main-old`)                        |
|-----------------------------|-------------------------------|------------------------------------------|
| Total LOC                   | 6 653                         | 5 560                                    |
| Production code (est.)      | ~3 750                        | ~4 700                                   |
| Test code (est.)            | ~2 900 (44 % of total)        | ~900 (16 % of total)                     |
| Test functions              | 149                           | 80                                       |
| Test files                  | 10 (separate `_test.go`)      | 13 (inline `#[cfg(test)]` modules)       |
| External packages           | 2 (`go-sqlite3`, `goqite`)    | 20+ crates                               |
| Runtime binary dependencies | 1 (`fetching-cli` on PATH)    | None (librespot linked directly)         |
| Build steps                 | `go build`                    | `npm run build` + `cargo build --release`|
| HTTP framework              | stdlib `net/http`             | Axum (tokio async)                       |
| Frontend                    | Server-rendered HTML templates| SPA / PWA (SvelteKit, embedded binary)   |
| Database                    | 2 tables via `goqite` + jobs  | 3 normalised tables (collections/tracks/tasks)|
| Progress state              | In-memory only (lost on restart)| DB-backed (survives restart)           |
| Per-track retry             | ✅ Yes (3 attempts + backoff) | ❌ No (re-enqueue only)                  |
| ReplayGain metadata         | ❌ No                         | ✅ Yes                                   |
| Cover art composite         | ✅ Yes (4-tile for playlists) | Partial (single track only at finalise)  |

---

## 1. Code Complexity

### Structure

The Go implementation is a **single module** with a flat `internal/` package tree. Each
package maps to one domain concept. All packages are wired together in a single 225-line
`main.go`. The absence of a module split is possible because Go never needs to link a
native audio library — that concern is delegated to the subprocess.

The Rust implementation is a **3-crate workspace**:

```
fetching/          ← binary, wires everything
  core/            ← librespot + download pipeline
  server/          ← Axum router + SSE + SPA assets
```

The workspace split exists because `librespot-core` adds heavy build-time dependencies
(OpenSSL, cmake, avahi). It is architecturally sound but adds a layer of indirection: types
must be `pub` in `core`, re-exported in `lib.rs`, and imported with the generated crate name
(`fetching_core_lib`) in the binary crate.

**Verdict:** Go's single-module layout is simpler to navigate. The Rust workspace split is
justified by the native dependency, but it adds friction for new contributors.

### Largest files

| File                        | Go LOC | File                        | Rust LOC |
|-----------------------------|--------|-----------------------------|----------|
| `worker/worker.go`          | 653    | `core/src/db.rs`            | 1 061    |
| `queue/queue.go`            | 456    | `core/src/coordinator.rs`   | 672      |
| `tagger/tagger.go`          | 321    | `core/src/runner.rs`        | 398      |
| `progress/store.go`         | 277    | `core/src/audio_librespot.rs`| 353     |
| `web/handler.go`            | 237    | `core/src/output_path.rs`   | 350      |

The Rust `db.rs` at 1 061 LOC is the standout: it carries the schema, migrations, all CRUD
helpers, row types, and status serialisation in one file. The Go queue module spreads a
similar concern across 456 LOC and delegates status to the separate `progress` package.

---

## 2. Dependency Management

### Go

```
github.com/mattn/go-sqlite3 v1.14.34   ← CGO binding, sqlite3 bundled
maragu.dev/goqite v0.4.0               ← visibility-timeout queue on SQLite
```

Two direct dependencies. The only C involvement is sqlite3 (via CGO). The flake pulls in
`fetching-cli` — a separately versioned binary from a different repository — as a
`wrapProgram` PATH injection. That binary is the true audio dependency.

### Rust

The workspace pulls 20+ direct crates including `librespot-core`, `librespot-audio`,
`librespot-metadata`, `librespot-oauth`, `tokio`, `axum`, `rusqlite` (bundled), `lofty`,
`moka`, `image`, `reqwest`, `serde`, `tracing`, `anyhow`, and more. Transitive dependency
count runs into the hundreds.

The dev shell requires `pkg-config`, `openssl`, `avahi`, `dbus`, and `cmake` in addition
to the Rust toolchain. An `npm`/Node.js toolchain is also required to compile the frontend.

**Verdict:** The Go dependency story is dramatically simpler. `go build` succeeds in a
vanilla environment with just Go and a C compiler. Rust requires a substantial native build
environment — acceptable for a developer tool on NixOS, but a meaningful barrier for
contributions on other platforms.

---

## 3. Architecture and Design Patterns

Both implementations share the same high-level structure:

```
HTTP handler  →  Queue  →  Worker loop  →  Download pipeline  →  Tagger  →  Playlist
```

The differences are in how each layer is expressed.

### The IPC boundary (Go) vs. linked library (Rust)

The most fundamental architectural difference is how librespot is integrated.

The **Go** version shells out to `fetching-cli` for every metadata fetch and audio download.
This provides clean process isolation and keeps the Go module dependency-free, but it
introduces a silent runtime dependency and adds serialisation overhead (credentials and audio
are piped via stdin/stdout/file descriptors for every call).

The **Rust** version compiles librespot directly. Authentication happens once per server
start; subsequent calls reuse the in-memory `Arc<Session>`. This is faster, more reliable,
and self-contained — but it ties the build to librespot's native dependency chain.

### Progress model

**Go** uses an in-memory `progress.Store` with a pub-sub fan-out to SSE clients. State is
per-job (collection + per-track) and very granular (7 `TrackStatus` values). A server
restart wipes the store — the web UI shows nothing until new jobs are submitted.

**Rust** uses a `tokio::sync::broadcast` channel for SSE and persists every status
transition to SQLite. On startup the server can serve the full collection history
immediately. The live progress data per track is coarser — primarily `Pending`, `Running`,
`Retrying`, `Done`, `Failed` — but is permanently available.

Both approaches are valid. Go's model is simpler to implement and test; Rust's is more
useful for long-running server deployments.

### Coordinator pattern (Rust) vs. Worker struct (Go)

Rust's `DownloadCoordinator` is an `Arc`-shared handle that encapsulates queue, database,
broadcaster, and the background worker as a single unit. Adding it to `AppState` gives
HTTP handlers access to `queue.add_collection()`, `queue.subscribe_progress()`, and
`queue.db()` through one reference.

Go's `Worker` is a separate struct that is started as a goroutine. HTTP handlers depend on
`*queue.Queue` and `*progress.Store` directly. The coordinator role is played implicitly by
`main.go`, which wires the two together.

The Rust approach is more self-contained; the Go approach is more explicit about which
component owns which responsibility.

---

## 4. Concurrency Model

| Aspect                    | Go                                  | Rust                                     |
|---------------------------|-------------------------------------|------------------------------------------|
| Runtime                   | Goroutines + `sync.Mutex`           | tokio multi-thread (3 worker threads)    |
| Worker scheduling         | Polling loop (2 s interval)         | `Notify`-based wake-up (no poll)         |
| Blocking I/O              | Goroutine blocks fine               | `spawn_blocking` for librespot calls     |
| Max concurrency           | `concurrency` flag (field stored, loop currently serial) | `Semaphore(1)`, trivially increasable |
| Context propagation       | `context.Context` through all calls | Implicit via tokio cancellation + Drop   |

The Go polling loop is a minor inefficiency: in serve mode, the worker wakes every 2 seconds
even when idle. A channel wake-up from `Enqueue` would eliminate this. The Rust `Notify`
primitive does exactly this.

The `Worker.concurrency` field in Go is declared and bounds-checked but the `Run` loop
does not launch per-job goroutines — it always processes one job at a time. The Rust
`Semaphore` approach makes the concurrency limit explicit and the code cleanly supports
increasing it.

---

## 5. Persistence and State

### Queue durability

Both implementations use SQLite. Go uses `goqite` (SQS-inspired visibility timeout) with an
auxiliary `jobs` table. Rust manages its own `collections` / `tracks` / `tasks` schema
directly via `rusqlite`.

| Feature                        | Go                          | Rust                          |
|--------------------------------|-----------------------------|-------------------------------|
| Crash recovery                 | ✅ `RecoverStuckJobs`        | ✅ `recover_interrupted`       |
| Visibility timeout (auto-retry)| ✅ goqite 15-min window      | ❌ Manual re-enqueue only      |
| Schema normalisation           | Flat (1 row = 1 job)        | Normalised (collection → tracks → tasks) |
| Progress survives restart      | ❌ In-memory only            | ✅ Full history in DB          |
| DB migration strategy          | Additive `ALTER TABLE`      | `CREATE TABLE IF NOT EXISTS`  |

The goqite visibility timeout is a meaningful safety net that the Rust implementation
does not replicate: if the server crashes mid-download, the job automatically re-appears in
the queue after 15 minutes, even without an explicit `recover_interrupted` call.

---

## 6. Testing

### Philosophy

Go follows the **separate test file** convention; tests are in `*_test.go` files that share
the package under test via `package worker` (white-box) or `package worker_test`
(black-box). Approximately 44% of total Go LOC is test code.

Rust co-locates tests in `#[cfg(test)]` modules at the bottom of source files, which is
idiomatic Rust. Approximately 16% of total Rust LOC is test code.

### Coverage depth

| Area                     | Go                                     | Rust                                   |
|--------------------------|----------------------------------------|----------------------------------------|
| Retry logic              | ✅ 5 dedicated tests                   | N/A (no retry logic)                   |
| Worker / coordinator     | ✅ Integration tests with real SQLite  | ✅ Async unit tests with stubs          |
| Queue persistence        | ✅ 30+ tests, real SQLite              | ✅ 20+ tests, in-memory SQLite          |
| HTTP handlers            | ✅ `httptest`-based                    | ❌ None                                 |
| Storage / path templates | ✅ Table-driven                        | ✅ 15+ cases for output_path            |
| Tagger                   | ✅ Real files                          | ✅ Lofty round-trips                    |
| Playlist writer          | ✅ Tested                              | ✅ Tested                               |
| Metadata parsing         | ✅ 40+ JSON fixtures                   | ✅ Container round-trips                |
| Download pipeline (E2E)  | ✅ Fake runner, real filesystem         | ❌ No tests for `DownloadRunner::run`   |
| Server / HTTP layer      | ✅ Full handler tests                  | ❌ No tests for `server/`              |

**Go** has broader integration testing and tests the HTTP surface. **Rust** has stronger
unit-level coordinator tests (async, multi-track, single-concurrency guarantees). Both have
gaps: Go in the assets generation path; Rust in the production runner and HTTP handlers.

---

## 7. Feature Parity

| Feature                        | Go                          | Rust                                |
|--------------------------------|-----------------------------|-------------------------------------|
| Album download                 | ✅                           | ✅                                   |
| Playlist download              | ✅                           | ✅                                   |
| Show / podcast download        | ✅                           | ✅                                   |
| Single track / episode         | ✅                           | ✅                                   |
| M3U8 playlist generation       | ✅                           | ✅                                   |
| Cover art (single)             | ✅                           | ✅ (written at download time)        |
| Cover art (composite playlist) | ✅ 4-tile montage            | ⚠️ Unimplemented in finaliser        |
| Embedded ID3 / Vorbis tags     | ✅ Via external tagger       | ✅ lofty (more formats)              |
| ReplayGain tags                | ❌                           | ✅                                   |
| ISRC / barcode / explicit flag | ❌                           | ✅                                   |
| Fallback quality               | ✅ `--fallback-quality`      | ✅ Candidate pool                    |
| Alternative track URIs         | ✅                           | ✅                                   |
| Per-track retry (transient)    | ✅ 3-attempt backoff         | ❌                                   |
| Crash recovery                 | ✅ Visibility timeout + explicit | ✅ Explicit only                  |
| Web UI                         | Server-rendered HTML        | SPA / PWA                           |
| Retry from web UI              | ✅ `POST /api/jobs/retry`    | ❌ Not exposed via API              |
| Log streaming                  | ✅ `GET /api/logs` + SSE     | ❌                                   |
| Configurable path templates    | ✅ Rich token set             | ❌ Fixed path structure             |
| History survives restart       | ❌                           | ✅                                   |
| NixOS module                   | ✅                           | ❌                                   |

The Go implementation prioritises operational features (retry, log visibility, path
templates, NixOS packaging). The Rust implementation prioritises metadata richness and
self-containedness.

---

## 8. Build and Setup Complexity

### Go

```sh
# development
go build ./cmd/fetching

# production (Nix)
nix build   # pulls fetching-cli automatically
```

Requirements: Go 1.24, GCC (for CGO/sqlite3). The `fetching-cli` binary comes from a
separate Nix input — its Rust build is handled in a different repository, so this project
never runs `cargo`.

### Rust

```sh
# frontend must be built first
cd frontend && npm install && npm run build && cd ..

# development
cargo build

# production (Nix)
nix build   # builds frontend then cargo
```

Requirements: Rust stable, pkg-config, OpenSSL, cmake, avahi, dbus, Node.js, npm.
The Nix flake also needs `rust-overlay` (adds a second flake input). A stale
`npmDepsHash` breaks the entire build.

**Verdict:** Go is dramatically easier to build in any environment. The Rust setup requires
a full native development environment and a separate frontend build pipeline. On NixOS with
the provided flake this is automated, but it is fragile (frontend hash churn) and slow
(librespot + frontend from scratch).

---

## 9. Overall Assessment

### Go strengths over Rust
- Minimal dependencies; builds with just `go build`
- Substantially more test coverage (149 vs 80 functions; 44% vs 16% LOC)
- HTTP layer is fully tested
- Configurable path templates
- Per-track retry with backoff
- goqite auto-recovery via visibility timeout
- In-process log streaming to frontend
- Retry from the web UI
- NixOS module

### Rust strengths over Go
- Self-contained: no runtime binary dependency, reproducible single binary
- Richer metadata: ReplayGain, ISRC, barcode, explicit, language
- Full history survives server restart
- More robust concurrency primitive (`Semaphore` vs polling)
- richer tag format support via `lofty` (MP3/OGG/FLAC/M4A)
- Cover art caching (`moka`)
- Full SPA frontend with more interactive UX

### Recommendation

For a **personal deployment on NixOS** (the primary use case, given the NixOS module in
main), the Go implementation is the better fit: it is easier to maintain, has significantly
better test coverage, handles failure cases more gracefully (per-track retry, UI retry
button, log streaming), and does not require a frontend build pipeline.

The Rust implementation is a better technical foundation if the goal is a **self-contained
downloadable binary** shipped to users who have Spotify credentials but no NixOS
infrastructure — its single-binary, no-external-deps story is compelling.

The most impactful improvements for each:

- **Go:** validate `fetching-cli` at startup; rebuild progress snapshot from DB on boot;
  replace the poll loop with a channel wake-up.
- **Rust:** add per-track retry; fix `find_track_file` (use exact path from DB); add HTTP
  handler tests; split `db.rs` into sub-modules.
