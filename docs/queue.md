# Queue Architecture

## Overview

The queue is split into two layers:

| Layer | File | Responsibility |
|---|---|---|
| **Types + Traits** | `core/src/queue.rs` | Data structures, `QueueStorage` (the replaceable seam), `JobRunner`, progress types |
| **Tokio runtime impl** | `core/src/queue_tokio.rs` | In-memory storage, Tokio channels, notify + semaphore, worker loop |

Future backends (sled, yaque) only need to implement `QueueStorage`; the Tokio scaffolding stays unchanged.

---

## Data Flow

```
  caller
    │
    │  add_collection(Arc<TrackCollection>)
    ▼
 TokioQueue
    │  pushes one QueueEntry per track_uri into QueueStorage
    │  wakes worker via Notify
    ▼
 worker loop
    │  pops entry from QueueStorage
    │  acquires Semaphore(1)  ← ensures single active download
    │  calls JobRunner::run(entry, apis) inside spawn_blocking
    │  sends ProgressUpdate to broadcast channel
    ▼
 broadcast::Receiver<ProgressUpdate>
    │  (SSE endpoint subscribes here)
```

---

## Key Types

### `QueueEntry`

One entry per **track URI** (not per collection). Holds:
- `task_id: Uuid` — stable identifier for SSE correlation
- `track_uri: String`
- `collection: Arc<TrackCollection>` — zero-copy shared reference to the parent collection

### `ProgressUpdate`

Minimal, opaque payload sent over SSE:
```rust
pub struct ProgressUpdate {
    pub task_id: Uuid,
    pub status: TaskStatus,     // Pending | Running | Done | Failed
    pub message: Option<String>,
}
```
Schema is intentionally kept narrow; richer fields can be added without breaking the backend seam.

### `WorkerApis`

Bundles all Spotify API handles. Constructed once, wrapped in `Arc`, cloned cheaply per worker task:
```rust
pub struct WorkerApis {
    pub collection_metadata: Arc<dyn SpotifyCollectionMetadata + Send + Sync>,
    pub track_metadata: Arc<dyn SpotifyTrackMetadata + Send + Sync>,
    pub cover: Arc<dyn CoverFetcher>,
}
```

`CoverFetcher` is a thin queue-internal trait (without the `Clone` supertrait that `SpotifyCover` carries), allowing object-safe `Arc<dyn CoverFetcher>`. Any `T: SpotifyCover` gets a blanket impl.

### `JobRunner`

The work-phase entry point. Not implemented here — the actual download/tag/store pipeline plugs in here later:
```rust
pub trait JobRunner: Send + Sync + 'static {
    fn run(&self, entry: &QueueEntry, apis: &WorkerApis) -> anyhow::Result<()>;
}
```
Because `JobRunner::run` is synchronous, the queue passes it to `spawn_blocking` so librespot's blocking audio code does not block the Tokio runtime.

---

## The Replaceable Seam: `QueueStorage`

```rust
pub trait QueueStorage: Send + Sync + 'static {
    fn push(&self, entry: QueueEntry) -> anyhow::Result<()>;
    fn pop(&self)  -> anyhow::Result<Option<QueueEntry>>;
}
```

Current implementation: `InMemoryStorage` (a `Mutex<VecDeque<QueueEntry>>`).

### Swapping to sled

1. Create `core/src/queue_sled.rs`.
2. Implement `QueueStorage` for `SledStorage`, serialising `QueueEntry` with `serde_json`/`bincode`.
3. Pass `Arc<SledStorage>` to `TokioQueue::with_storage(...)` instead of `Arc<InMemoryStorage>`.
4. Everything else (worker loop, semaphore, progress broadcast) is unchanged.

### Swapping to yaque

1. Create `core/src/queue_yaque.rs`.
2. yaque is async, so implement `QueueStorage` using `block_on` or a shared `Handle::block_on` inside `push`/`pop`.
3. Alternatively, expose an async `YaqueBackend` alongside the sync trait and wire it in `TokioQueue` via a `tokio::task::spawn_blocking` wrapper.

---

## Concurrency Contract

- The Tokio `Semaphore(1)` guarantees **at most one active `JobRunner::run` call** at any time.
- Multiple entries may be queued; they will execute strictly one-at-a-time in FIFO order.
- `spawn_blocking` keeps librespot's blocking calls off the async executor.

---

## Implementation Plan

- [x] `docs/queue.md` — this file
- [x] Add `uuid` (with `v4`, `serde` features) to `core/Cargo.toml`
- [x] `core/src/queue.rs` — `QueueEntry`, `TaskStatus`, `ProgressUpdate`, `WorkerApis`, `CoverFetcher`, `JobRunner`, `QueueStorage`
- [x] `core/src/queue_tokio.rs` — `InMemoryStorage`, `TokioQueue`, worker loop
- [x] `core/src/lib.rs` — `pub mod queue; pub mod queue_tokio;`
- [x] `cargo build` green
