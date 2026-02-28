# Redesign: Relational DB + REST/SSE

**Status**: Agreed, implementation in progress  
**Date**: 2026-02-28

## TL;DR

Replace sled with **rusqlite (bundled) SQLite**. Normalize data into `collections`, `tracks`, `tasks` tables. Rework API to **targeted REST endpoints** + **lightweight SSE deltas** patched in-place. Runner drops in-memory `CollectionState` in favor of DB queries for finalization (crash-safe, stateless). Cover art stays as-is. Clean break from sled.

**Edge case noted**: A track URI can appear in multiple collections (e.g., a `SingleTrack` collection *and* the full album). The schema supports this via the `(uri, collection_id)` unique constraint — same track URI gets separate rows per collection, each with its own task. This is correct for now; a future optimization could deduplicate downloads (skip audio fetch if another task for the same URI already succeeded) but that's out of scope.

---

## Database Schema

Three normalized tables in `fetching.db`:

### `collections`
- `id` TEXT PK (UUID)
- `uri` TEXT UNIQUE NOT NULL
- `collection_type` TEXT NOT NULL
- `title` TEXT NOT NULL
- `artists` TEXT NOT NULL (JSON array)
- `cover_id` TEXT
- `upc`, `label`, `date` TEXT
- `total_tracks` INTEGER NOT NULL
- `registered_at` TEXT NOT NULL (ISO 8601)

### `tracks`
- `id` TEXT PK (UUID)
- `uri` TEXT NOT NULL
- `collection_id` TEXT NOT NULL FK → collections(id)
- `title`, `artists` (JSON), `cover_id`, `isrc`, `date` TEXT — *nullable until resolved*
- `duration_ms`, `disc_number`, `number` INTEGER — *nullable until resolved*
- `explicit` INTEGER DEFAULT 0
- `language` TEXT (JSON array)
- UNIQUE(`uri`, `collection_id`) — allows same track URI in different collections

### `tasks`
- `id` TEXT PK (UUID = task_id)
- `track_id` TEXT NOT NULL FK → tracks(id)
- `status` TEXT NOT NULL DEFAULT 'pending'
- `message` TEXT
- `registered_at` TEXT NOT NULL (ISO 8601)

**Indexes**: `tasks(status)`, `collections(registered_at)`, `tracks(collection_id)`, `tracks(uri)`

---

## REST API

| Method | Path | Returns | Purpose |
|--------|------|---------|---------|
| `POST` | `/api/queue` | `{ collection_id, track_ids[], task_ids[] }` | Enqueue Spotify URL → insert collection + tracks + tasks |
| `GET` | `/api/collections` | `CollectionRow[]` with aggregated `status`, `progress` | List all collections, sorted by `registered_at` DESC |
| `GET` | `/api/collections/{id}/tracks` | `TrackRow[]` joined with task status | Tracks for one collection. Called on tile expand |
| `GET` | `/events` | SSE stream | Delta events: `{ task_id, collection_id, status, message, track_info? }` |
| `GET` | `/api/status` | Pending count | Keep existing |

Collection-level `status`/`progress` computed via SQL aggregation over tasks.

---

## Implementation Steps

### Backend — Database layer

1. Add `rusqlite` with `bundled` feature to `core/Cargo.toml`. Remove `sled` dependency.

2. Create `core/src/db.rs` — `Database` struct wrapping `Mutex<rusqlite::Connection>`. Methods:
   - `open(path)` — create file, run `CREATE TABLE IF NOT EXISTS` + indexes, enable WAL mode
   - `insert_collection(TrackCollection) -> collection_id`
   - `insert_track(track_uri, collection_id) -> track_id`
   - `insert_task(track_id) -> task_id`
   - `update_task(task_id, status, message)`
   - `update_track_metadata(track_id, Track)` — fills nullable columns after runner resolves metadata
   - `list_collections() -> Vec<CollectionRow>` — aggregated status/progress via SQL
   - `get_tracks_for_collection(collection_id) -> Vec<TrackRow>` — join tracks + tasks
   - `find_existing_task(track_uri, collection_id) -> Option<task_id>` — dedup check (indexed)
   - `recover_interrupted()` — `UPDATE tasks SET status='pending' WHERE status IN ('running','retrying')`
   - `is_collection_complete(collection_id) -> bool` — `SELECT COUNT(*) FROM tasks ... WHERE status != 'done'`
   - `get_collection_track_paths(collection_id) -> Vec<(number, title, path)>` — for M3U8 generation

3. Delete `core/src/registry.rs` — `TaskRegistry` trait, `SledRegistry`, `StoredTask`, `TaskSnapshot` all replaced by `Database`. Keep `TaskStatus` enum (move to `db.rs` or shared `types.rs`). Keep `ProgressUpdate` as SSE broadcast payload.

### Backend — Coordinator

4. Update `core/src/coordinator.rs`:
   - Replace `Option<Box<dyn TaskRegistry>>` with `Arc<Database>`
   - `add_collection()` → insert into all three tables via `Database`, return IDs
   - `emit_update()` → `db.update_task()` + broadcast SSE delta
   - Remove `snapshot()` — handlers query DB directly
   - `recover_interrupted()` → delegate to DB, re-queue returned task IDs into in-memory `TrackQueue`

### Backend — Runner (stateless finalization)

5. Update `core/src/runner.rs`:
   - After resolving track metadata: call `db.update_track_metadata(track_id, track)`
   - After a track completes (`Done`): call `db.is_collection_complete(collection_id)`
     - If complete → query `db.get_collection_track_paths(collection_id)`, generate M3U8 + cover.jpg
   - Remove `CollectionState`, `TrackEntry`, and the `collections` `Mutex<HashMap>`

6. Add `collection_id` to `ProgressUpdate` so frontend can route SSE events without a lookup map.

### Backend — Server

7. Update `server/src/server.rs`:
   - `AppState` holds `Arc<Database>`
   - Wire new routes: `GET /api/collections`, `GET /api/collections/:id/tracks`
   - Remove `GET /api/queue`

8. Update `server/src/handlers.rs`:
   - `POST /api/queue` → resolve metadata, insert via `Database`, return `{ collection_id, track_ids, task_ids }`
   - `GET /api/collections` → `db.list_collections()`
   - `GET /api/collections/:id/tracks` → `db.get_tracks_for_collection(id)`
   - SSE endpoint unchanged

### Frontend — Types & API

9. Update `frontend/src/lib/types.ts`:
   - `CollectionItem`: `{ id, uri, type, title, artists, cover_id, date, total_tracks, status, progress, registered_at }`
   - `TrackItem`: nullable fields for skeleton rows
   - `SseEvent`: add `collection_id`
   - Remove `QueueResponse`, `QueueItem`

10. Rewrite `frontend/src/lib/api.ts`:
    - `fetchCollections()` → `GET /api/collections`
    - `fetchTracks(collectionId)` → `GET /api/collections/{id}/tracks`
    - `queueUrl(url)` → `POST /api/queue`
    - Remove `responseToQueueItem()`

### Frontend — Page & Components

11. Rewrite SSE handling in `+page.svelte`:
    - On SSE event: use `collection_id` to find matching `CollectionItem`, patch in-place
    - No full refresh ever. UI state preserved.

12. Update `QueueView.svelte`:
    - On expand: `fetchTracks(collectionId)`, cache in `Map<string, TrackItem[]>`
    - On collapse: keep cache

13. Update `TrackList.svelte`:
    - Skeleton rows when `title` is null
    - Reactively patched per-track via SSE

14. Update `mock.ts` to match new API shapes.

### Cleanup

15. Remove sled handling from `main.rs`. Log info if `queue.sled/` found.
16. Update `core/src/lib.rs` exports.
17. Update `docs/queue.md`.

---

## Decisions

- **rusqlite (bundled)** — traditional, synchronous, zero system deps
- **REST + targeted SSE** — real-time without state-clobbering
- **`collection_id` in SSE payload** — removes need for client-side lookup map
- **Stateless runner finalization via DB** — crash-safe, no in-memory CollectionState
- **Cover art unchanged** — defer to future pass
- **Clean break** — no sled migration
- **Keep all history** — no pruning
- **Skeleton rows** for unresolved tracks
- **Multi-collection tracks deferred** — schema supports it, download dedup is future work
