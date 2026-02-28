# Queue Architecture

## Overview

The queue system uses a SQLite database (`fetching.db`) as the persistent store for all collection and track state. The architecture is split into three layers:

| Layer | File | Responsibility |
|---|---|---|
| **Types** | `core/src/queue.rs` | `QueueEntry`, `TaskId` — in-memory queue entry types |
| **Database** | `core/src/db.rs` | `Database` struct — SQLite-backed persistence (collections, tracks, tasks) |
| **Coordinator** | `core/src/coordinator.rs` | `DownloadCoordinator` — orchestration, worker loop, SSE broadcast |

---

## Data Flow

```
  caller (POST /api/queue)
    │
    │  add_collection(Arc<TrackCollection>)
    ▼
 DownloadCoordinator
    │  inserts collection + tracks + tasks into Database (SQLite)
    │  pushes QueueEntry per track_uri into in-memory queue
    │  wakes worker via Notify
    ▼
 worker loop
    │  pops entry from in-memory queue
    │  acquires Semaphore(1)  ← ensures single active download
    │  calls JobRunner::run(entry, apis, collection_id) inside spawn_blocking
    │  sends ProgressUpdate (with collection_id) to broadcast channel
    │  updates task status in Database
    ▼
 broadcast::Receiver<ProgressUpdate>
    │  (SSE endpoint subscribes here)
    │  Frontend patches individual tracks in-place using collection_id + task_id
```

---

## Database Schema (SQLite)

Three normalized tables:

- **collections** — one row per album/playlist/single queued
- **tracks** — one row per track URI within a collection (nullable metadata until resolved)
- **tasks** — one row per download task (status, message, timestamps)

Key indexes: `idx_tasks_status`, `idx_tasks_registered`, `idx_tracks_collection`.

See `core/src/db.rs` for the full schema and `docs/redesign-relational-db.md` for the design rationale.

---

## Key Types

### `QueueEntry`

One entry per **track URI** (not per collection). Holds:
- `task_id: Uuid` — stable identifier for SSE correlation
- `track_uri: String`
- `collection: Arc<TrackCollection>` — zero-copy shared reference to the parent collection

### `ProgressUpdate`

Payload sent over SSE:
```rust
pub struct ProgressUpdate {
    pub task_id: Uuid,
    pub collection_id: String,   // added: targets SSE patching
    pub status: TaskStatus,      // Pending | Running | Retrying | Done | Failed
    pub message: Option<String>,
    pub track_info: Option<TrackInfo>,
}
```

### `WorkerApis`

Bundles all Spotify API handles:
```rust
pub struct WorkerApis {
    pub collection_metadata: Arc<dyn SpotifyCollectionMetadata + Send + Sync>,
    pub track_metadata: Arc<dyn SpotifyTrackMetadata + Send + Sync>,
    pub cover: Arc<dyn CoverFetcher>,
    pub audio: Arc<dyn AudioDownloader + Send + Sync>,
}
```

### `JobRunner`

The work-phase entry point:
```rust
pub trait JobRunner: Send + Sync + 'static {
    fn run(
        &self,
        entry: &QueueEntry,
        apis: &WorkerApis,
        collection_id: &str,
        progress: &dyn Fn(ProgressUpdate),
    ) -> anyhow::Result<Option<String>>;
}
```

---

## REST API

| Method | Path | Description |
|---|---|---|
| POST | `/api/queue` | Enqueue a Spotify URL; returns `{collection_id, track_ids, task_ids}` |
| GET | `/api/collections` | List all collections with aggregate status counts |
| GET | `/api/collections/{id}/tracks` | Tracks + task status for one collection |
| GET | `/api/status` | Health check |
| GET | `/events` | SSE stream of `ProgressUpdate` events |

---

## Concurrency Contract

- The Tokio `Semaphore(1)` guarantees **at most one active `JobRunner::run` call** at any time.
- Multiple entries may be queued; they will execute strictly one-at-a-time in FIFO order.
- `spawn_blocking` keeps librespot's blocking calls off the async executor.

---

## Recovery

On startup, `Database::recover_interrupted()` resets any `running`/`retrying` tasks back to `pending` and returns them for re-queuing. This makes the system crash-safe — if the process dies mid-download, those tasks will be retried on next launch.
