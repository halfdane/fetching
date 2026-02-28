# Major agentic redesign session

## Initial want
User: okay, I've had enough with the current approach to the frontend: it keeps losing its state, doesn't refresh tracks properly, and the frontend/backend integration using SSE feels clunky.

I'd like to move to a more traditional communication, based on the concept of the taskRegistry

Frontend:
- initial load fetches collections that have a status, cover, some other metadata, and a list of track-ids
- when expanding the album's tile, the necessary data to display the tracks, is fetched based on available track-ids.
- during runtime, the updated data for tracks and albums is fetched from the backend according to their id
-> almost no frontend state (state-sync is *hard*)

backend:
- since the whole task registry is designed like a traditional relational database, it doesn't make sense to use a binary tree implementation
- makes it hard to sort by date or other criteria
- dependent data is deeply integrated and consequently duplicated (another collection per track)
- I propose to switch to a simple relational embedded db that handles all that.

It's a major redesign, and I'm not really sure if I got all details right, so I'd be happy to get your input.

We're in the design phase (no code changes) until I say we're ready.

GitHub Copilot: 🍀 💡 This is a significant architectural redesign — let me thoroughly research the current codebase before we discuss the plan. I'll start with discovery. 

Deep codebase architecture research

Read [](file:///home/user/halfdane/fetching/core/src/lib.rs)

Read [](file:///home/user/halfdane/fetching/core/src/registry.rs#1-1), lines 1 to 500

Read [](file:///home/user/halfdane/fetching/core/src/container.rs)

Read [](file:///home/user/halfdane/fetching/core/src/coordinator.rs#1-1), lines 1 to 500

Read [](file:///home/user/halfdane/fetching/core/src/runner.rs)

Read [](file:///home/user/halfdane/fetching/core/src/queue.rs)

Read [](file:///home/user/halfdane/fetching/core/src/queue_memory.rs)

Read [](file:///home/user/halfdane/fetching/core/src/playlist.rs)

Read [](file:///home/user/halfdane/fetching/core/src/spotify_api.rs)

Read [](file:///home/user/halfdane/fetching/core/src/tagger.rs)

Read [](file:///home/user/halfdane/fetching/core/src/audio.rs)

Read [](file:///home/user/halfdane/fetching/core/src/audio_librespot.rs)

Read [](file:///home/user/halfdane/fetching/core/src/output_path.rs)

Read [](file:///home/user/halfdane/fetching/core/Cargo.toml)

Read [](file:///home/user/halfdane/fetching/server/src/lib.rs)

Read [](file:///home/user/halfdane/fetching/server/src/server.rs)

Read [](file:///home/user/halfdane/fetching/server/src/handlers.rs)

Read [](file:///home/user/halfdane/fetching/server/src/assets.rs)

Read [](file:///home/user/halfdane/fetching/server/Cargo.toml)

Read [](file:///home/user/halfdane/fetching/Cargo.toml)

Read [](file:///home/user/halfdane/fetching/main.rs)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/api.ts)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/types.ts)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/index.ts)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/QueueView.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/TrackList.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/AddToQueue.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/Toast.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/DevDrawer.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/routes/%2Bpage.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/routes/%2Blayout.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/mock.ts)

Read [](file:///home/user/halfdane/fetching/docs/queue.md)

Read [](file:///home/user/halfdane/fetching/docs/security.md)

Read [](file:///home/user/halfdane/fetching/docs/specification.md)

Read [](file:///home/user/halfdane/fetching/docs/token-refresh-architecture.md)

Read [](file:///home/user/halfdane/fetching/docs/ux_vision.md)

Read [](file:///home/user/halfdane/fetching/core/src/librespot_impl/mod.rs)

Read [](file:///home/user/halfdane/fetching/core/src/librespot_impl/session.rs)

Read [](file:///home/user/halfdane/fetching/core/src/librespot_impl/track_metadata.rs)

Read [](file:///home/user/halfdane/fetching/core/src/librespot_impl/collection_metadata.rs)

Read [](file:///home/user/halfdane/fetching/core/src/librespot_impl/cover_fetcher.rs)

Read [](file:///home/user/halfdane/fetching/core/src/librespot_impl/cached_cover_fetcher.rs)

Read [](file:///home/user/halfdane/fetching/core/src/registry.rs#500-500), lines 500 to 553

Read [](file:///home/user/halfdane/fetching/core/src/coordinator.rs#500-500), lines 500 to 615

Read [](file:///home/user/.config/Code/User/workspaceStorage/9a4f2e67e51eac071abf53a155c3ef41/GitHub.copilot-chat/chat-session-resources/9dbc84f0-16f7-4d40-952a-3b1e8d665039/toolu_019Psm9d9tSvND9gSoZZQZ4B__vscode-1772205791155/content.txt)

Great research complete. I have a thorough picture of the current architecture. Let me surface some findings and clarify a few design decisions before drafting a plan.

**Key Pain Points Confirmed:**

1. **State loss root cause**: Every SSE event triggers a debounced full `GET /api/queue` which *replaces the entire `queue` array*. Any ephemeral UI state (expanded cards, scroll position) is blown away.
2. **Data duplication in sled**: Each `StoredTask` serializes the *entire* `Arc<TrackCollection>` (with all track URIs). A 20-track album means 20 copies of the same collection JSON.
3. **Cover art re-fetched on every snapshot**: `GET /api/queue` calls `spawn_blocking` + librespot cover fetch per collection on every request (moka cache mitigates, but still).
4. **O(n) dedup scan**: `SledRegistry::register()` iterates all entries to check for duplicate `track_uri`.
5. **No relational queries**: Can't efficiently sort by date, filter by status, or query tracks by collection without loading everything. 

Asked 4 questions (DB Choice, API Style, Cover Art, Migration)

Good answers. "Traditional and non-exciting" = **rusqlite + SQLite** — exactly right. Let me clarify a few more model-level details: 

Asked 4 questions (Schema, SSE Scope, Track Fetch, History)

All decisions locked in. Here's the design:

---

## Plan: Relational DB + REST/SSE Redesign

### TL;DR

Replace the sled-backed `TaskRegistry` with a **rusqlite SQLite** database using a normalized schema (`collections`, `tracks`, `tasks`). Rework the API from "dump everything + full-refresh on SSE" to **targeted REST endpoints** (collections list, track details by collection) plus **lightweight SSE deltas** that the frontend patches in-place. This eliminates the state-loss problem (no more replacing the whole queue array), removes data duplication (collection stored once, not per-track), and enables proper sorting/filtering via SQL. Cover art handling stays as-is for now. Sled is removed entirely (clean break, no migration).

---

### New Database Schema

Three normalized tables in a single `fetching.db` SQLite file:

**`collections`** — one row per album/playlist/show
- `id` TEXT PRIMARY KEY (UUID)
- `uri` TEXT UNIQUE NOT NULL
- `collection_type` TEXT NOT NULL (album/playlist/show/single_track/single_episode)
- `title` TEXT NOT NULL
- `artists` TEXT NOT NULL (JSON array)
- `cover_id` TEXT
- `upc` TEXT
- `label` TEXT
- `date` TEXT
- `total_tracks` INTEGER NOT NULL
- `registered_at` TEXT NOT NULL (ISO 8601)

**`tracks`** — one row per track, FK to collection. Starts sparse (just URI), filled when runner resolves metadata.
- `id` TEXT PRIMARY KEY (UUID)
- `uri` TEXT NOT NULL
- `collection_id` TEXT NOT NULL REFERENCES collections(id)
- `title` TEXT  *(nullable — unknown until resolved)*
- `artists` TEXT  *(JSON array, nullable)*
- `cover_id` TEXT
- `isrc` TEXT
- `duration_ms` INTEGER
- `disc_number` INTEGER
- `number` INTEGER
- `date` TEXT
- `explicit` INTEGER DEFAULT 0
- `language` TEXT  *(JSON array)*
- UNIQUE(`uri`, `collection_id`)

**`tasks`** — one row per download task, FK to track. This is the status/progress record.
- `id` TEXT PRIMARY KEY (UUID — the `task_id`)
- `track_id` TEXT NOT NULL REFERENCES tracks(id)
- `status` TEXT NOT NULL DEFAULT 'pending'
- `message` TEXT
- `registered_at` TEXT NOT NULL (ISO 8601)

Indexed: `tasks(status)`, `collections(registered_at)`, `tracks(collection_id)`, `tracks(uri)`.

---

### New REST API

| Method | Path | Returns | Purpose |
|--------|------|---------|---------|
| `POST` | `/api/queue` | `{ collection_id, track_ids[], task_ids[] }` | Enqueue a Spotify URL. Resolves metadata, inserts collection + tracks + tasks. |
| `GET` | `/api/collections` | `[{ id, uri, type, title, artists, cover_id, date, total_tracks, status, progress, registered_at }]` | List all collections, sorted by `registered_at` DESC. `status` and `progress` derived via SQL (aggregate over task statuses). |
| `GET` | `/api/collections/{id}/tracks` | `[{ id, uri, title, artists, number, disc_number, duration_ms, task_id, status, message }]` | Fetch tracks + their task status for a specific collection. Called when user expands a tile. |
| `GET` | `/api/tracks/{id}` | Single track + task detail | *Optional, for future use* — fetch one track by ID. |
| `GET` | `/events` | SSE stream | Small delta events (unchanged format: `{ task_id, status, message, track_info? }`) |
| `GET` | `/api/status` | Pending count | Health/status (keep existing) |

**Key change**: `GET /api/collections` replaces `GET /api/queue`. It does a single SQL query with aggregation — no need to load + group + re-derive in Rust code. Collection-level status is computed via SQL:
```
CASE
  WHEN COUNT(*) FILTER (WHERE status IN ('running','retrying')) > 0 THEN 'running'
  WHEN COUNT(*) FILTER (WHERE status = 'done') = COUNT(*) THEN 'done'
  WHEN COUNT(*) FILTER (WHERE status = 'failed') > 0 THEN 'failed'
  ELSE 'pending'
END
```

---

### Backend Changes

**Steps:**

1. **Add `rusqlite` dependency** to Cargo.toml with `bundled` feature (bundles SQLite, no system dependency needed on NixOS).

2. **Create a new `db.rs` module** in src implementing a `Database` struct wrapping a `rusqlite::Connection` (behind a `Mutex` for thread safety). Methods:
   - `open(path)` — creates file, runs `CREATE TABLE IF NOT EXISTS` migrations
   - `insert_collection(collection) -> collection_id`
   - `insert_track(track_uri, collection_id) -> track_id`
   - `insert_task(track_id) -> task_id`
   - `update_task(task_id, status, message)`
   - `update_track_metadata(track_id, Track)` — fills in the nullable columns
   - `list_collections() -> Vec<CollectionRow>` — with aggregated status/progress
   - `get_tracks_for_collection(collection_id) -> Vec<TrackRow>` — joins tracks + tasks
   - `recover_interrupted()` — SQL: `UPDATE tasks SET status='pending' WHERE status IN ('running','retrying')`
   - `find_existing_task(track_uri, collection_id) -> Option<task_id>` — for dedup

3. **Replace `TaskRegistry` trait and `SledRegistry`** with the `Database` struct. The registry.rs file becomes the new `db.rs`, or `registry.rs` is refactored to delegate to `Database`. The trait methods (`register`, `update`, `snapshot`) are replaced by the more granular DB methods above.

4. **Update `DownloadCoordinator`** in coordinator.rs:
   - Replace `Option<Box<dyn TaskRegistry>>` with `Arc<Database>`
   - `add_collection()` → inserts into all three tables, returns IDs
   - `emit_update()` → calls `db.update_task()` + broadcasts SSE delta
   - `snapshot()` → removed (replaced by direct DB queries from handlers)

5. **Update `DownloadRunner`** in runner.rs:
   - After resolving track metadata, call `db.update_track_metadata(track_id, track)` to persist the resolved fields
   - The `ProgressUpdate` for SSE continues to carry `track_info` so the frontend can patch immediately without waiting for a REST call

6. **Remove sled dependency** from Cargo.toml and delete `SledRegistry`, `StoredTask`, and all sled-related code.

7. **Update server handlers** in handlers.rs:
   - `POST /api/queue` → resolve metadata, insert via `Database`, return IDs
   - New `GET /api/collections` → `db.list_collections()`
   - New `GET /api/collections/{id}/tracks` → `db.get_tracks_for_collection(id)`
   - SSE endpoint stays the same (broadcast channel, delta events)
   - `GET /api/queue` → removed

8. **Update `AppState`** in server.rs to hold `Arc<Database>` and wire up new routes.

---

### Frontend Changes

**Steps:**

9. **Update types** in types.ts:
   - `CollectionItem` replaces `QueueItem`: `{ id, uri, type, title, artists, cover_id, date, total_tracks, status, progress, registered_at }`
   - `TrackItem` stays similar but comes from the `/tracks` endpoint: `{ id, uri, title, artists, number, disc_number, duration_ms, task_id, status, message }`
   - Remove `QueueResponse`, `SseEvent` keeps its shape

10. **Rewrite API layer** in api.ts:
    - `fetchCollections()` → `GET /api/collections` — returns `CollectionItem[]`
    - `fetchTracks(collectionId)` → `GET /api/collections/{id}/tracks` — returns `TrackItem[]`
    - `queueUrl(url)` → `POST /api/queue` — returns `{ collection_id, track_ids, task_ids }`
    - Remove `responseToQueueItem()` (status derivation now done server-side)

11. **Rewrite SSE handling** in +page.svelte:
    - On SSE event: find the matching `CollectionItem` by looking up which collection owns `task_id` (maintain a `Map<task_id, collection_id>` built from initial load + enqueue responses)
    - Patch the collection's `status`/`progress` in-place (recalculate from known track statuses, or re-fetch just that collection)
    - If a track list is expanded and the event matches a visible track, update that track's status/message in-place
    - **If `track_info` is present**: update the skeleton track row with real metadata (title, artists, duration) — no need to re-fetch
    - **No more full refresh**. Scroll position, expanded state, everything preserved.

12. **Update `QueueView.svelte`** in QueueView.svelte:
    - Renamed data prop from `queue` to `collections`
    - On card expand: call `fetchTracks(collectionId)`, cache result in a `Map<collection_id, TrackItem[]>`
    - On card collapse: keep cached tracks (no re-fetch on re-expand unless stale)
    - Track list shows skeleton rows for unresolved tracks (URI visible, status dot, placeholder for title/duration)

13. **Update `TrackList.svelte`** in TrackList.svelte:
    - Handle `null` title/artists/duration gracefully (show skeleton/placeholder)
    - Reactively update individual rows when SSE patches arrive

14. **Update mock data** in mock.ts to match new API shape. Remove dead `spotify_id`/`popularity` fields.

---

### Verification

- **Unit tests**: `Database` methods — insert, update, list, recovery, dedup. Pure rusqlite with an in-memory DB (`:memory:`).
- **Integration test**: Coordinator + Database + mock runner — enqueue a collection, verify tasks appear in DB, simulate status updates, verify `list_collections` returns correct aggregated status.
- **Frontend smoke test**: Load page → verify collections render → expand one → verify tracks load → simulate SSE event → verify in-place update without state loss.
- **Migration safety**: Since it's a clean break, just verify the app starts fresh without a queue.sled directory. Optionally log a warning if queue.sled exists ("legacy data found, you may delete queue.sled/").

---

### Decisions

- **rusqlite (bundled)** over sqlx/diesel — "traditional and non-exciting", synchronous API pairs naturally with `spawn_blocking`, no async ORM complexity
- **REST + targeted SSE** over pure polling or WebSocket — keeps real-time feel without the state-clobbering full-refresh
- **Cover art unchanged** — current moka-cached approach stays; can be moved to DB/disk in a future pass
- **Clean break** from sled — no migration, existing queue data abandoned
- **Keep history** — no pruning of finished tasks/collections
- **Skeleton rows** for unresolved tracks — shows progress, avoids jarring layout shifts when metadata arrives
- **`task_id → collection_id` map** on frontend — lightweight bookkeeping to route SSE events to the right collection without a round-trip

---

### Open Consideration

One thing worth flagging: the current `DownloadRunner` manages per-collection finalization (M3U8 playlist + cover.jpg) using an in-memory `CollectionState` with a `HashMap` behind a `Mutex`. With the new DB, this "is last track done?" check becomes a simple SQL query (`SELECT COUNT(*) FROM tasks WHERE track_id IN (SELECT id FROM tracks WHERE collection_id = ?) AND status != 'done'`). This would let the runner drop the in-memory `CollectionState` entirely, making it stateless and crash-safe. I'd recommend folding this into the plan, but it can also be a follow-up. Your call.

## Some clarifications

User: - fold this into the plan.
- regarding `/api/collections/{id}/tracks`: tracks *might* be in several collections due to the concept of SingleTrackCollections (where just one track of an album/playlist/show is fetched) that *may* be downloaded in addition to the whole album containing the single track. It's an edge case, sure, and I'm fine shoving that to later, but I thought you should know 

GitHub Copilot: 

🍀 Good catch on the shared tracks. Let me incorporate both points and flag the multi-collection track edge case properly.

---

## Plan: Relational DB + REST/SSE Redesign (v2)

### TL;DR

Replace sled with **rusqlite (bundled) SQLite**. Normalize data into `collections`, `tracks`, `tasks` tables. Rework API to **targeted REST endpoints** + **lightweight SSE deltas** patched in-place. Runner drops in-memory `CollectionState` in favor of DB queries for finalization (crash-safe, stateless). Cover art stays as-is. Clean break from sled.

**Edge case noted**: A track URI can appear in multiple collections (e.g., a `SingleTrack` collection *and* the full album). The schema supports this via the `(uri, collection_id)` unique constraint — same track URI gets separate rows per collection, each with its own task. This is correct for now; a future optimization could deduplicate downloads (skip audio fetch if another task for the same URI already succeeded) but that's out of scope.

---

### Database Schema

Three normalized tables in `fetching.db`:

**`collections`**
- `id` TEXT PK (UUID)
- `uri` TEXT UNIQUE NOT NULL
- `collection_type` TEXT NOT NULL
- `title` TEXT NOT NULL
- `artists` TEXT NOT NULL (JSON array)
- `cover_id` TEXT
- `upc`, `label`, `date` TEXT
- `total_tracks` INTEGER NOT NULL
- `registered_at` TEXT NOT NULL (ISO 8601)

**`tracks`**
- `id` TEXT PK (UUID)
- `uri` TEXT NOT NULL
- `collection_id` TEXT NOT NULL FK → collections(id)
- `title`, `artists` (JSON), `cover_id`, `isrc`, `date` TEXT — *nullable until resolved*
- `duration_ms`, `disc_number`, `number` INTEGER — *nullable until resolved*
- `explicit` INTEGER DEFAULT 0
- `language` TEXT (JSON array)
- UNIQUE(`uri`, `collection_id`) — allows same track URI in different collections

**`tasks`**
- `id` TEXT PK (UUID = task_id)
- `track_id` TEXT NOT NULL FK → tracks(id)
- `status` TEXT NOT NULL DEFAULT 'pending'
- `message` TEXT
- `registered_at` TEXT NOT NULL (ISO 8601)

**Indexes**: `tasks(status)`, `collections(registered_at)`, `tracks(collection_id)`, `tracks(uri)`

---

### REST API

| Method | Path | Returns | Purpose |
|--------|------|---------|---------|
| `POST` | `/api/queue` | `{ collection_id, track_ids[], task_ids[] }` | Enqueue Spotify URL → insert collection + tracks + tasks |
| `GET` | `/api/collections` | `CollectionRow[]` with aggregated `status`, `progress` | List all collections, sorted by `registered_at` DESC |
| `GET` | `/api/collections/{id}/tracks` | `TrackRow[]` joined with task status | Tracks for one collection. Called on tile expand |
| `GET` | `/events` | SSE stream | Delta events: `{ task_id, status, message, track_info? }` |
| `GET` | `/api/status` | Pending count | Keep existing |

Collection-level `status`/`progress` computed via SQL aggregation over tasks (no Rust-side derivation needed).

---

### Steps

**Backend — Database layer**

1. Add `rusqlite` with `bundled` feature to Cargo.toml. Remove `sled` dependency.

2. Create core/src/db.rs — `Database` struct wrapping `Mutex<rusqlite::Connection>`. Methods:
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
   - `get_collection_track_paths(collection_id) -> Vec<(number, title, path)>` — for M3U8 generation after finalization

3. Delete registry.rs — `TaskRegistry` trait, `SledRegistry`, `StoredTask`, `TaskSnapshot` all replaced by `Database` + row types. Keep `TaskStatus` enum (move to `db.rs` or a shared `types.rs`). Remove `ProgressUpdate` struct if its fields are subsumed by DB method signatures, or keep it as the SSE broadcast payload.

**Backend — Coordinator**

4. Update coordinator.rs:
   - Replace `Option<Box<dyn TaskRegistry>>` with `Arc<Database>`
   - `add_collection()` → insert into all three tables via `Database`, return IDs
   - `emit_update()` → `db.update_task()` + broadcast SSE delta (keep `broadcast::Sender<ProgressUpdate>`)
   - Remove `snapshot()` — handlers query DB directly
   - `recover_interrupted()` → delegate to `db.recover_interrupted()`, re-queue returned task IDs into in-memory `TrackQueue`

**Backend — Runner (stateless finalization)**

5. Update runner.rs:
   - After resolving track metadata: call `db.update_track_metadata(track_id, track)` to persist
   - After a track completes (`Done`): call `db.is_collection_complete(collection_id)`
     - If complete → query `db.get_collection_track_paths(collection_id)`, generate M3U8 playlist + write cover.jpg
     - This replaces the in-memory `CollectionState` (`HashMap<String, CollectionState>` behind `Mutex`) entirely
   - **Crash-safety benefit**: if the process dies mid-collection, the DB knows which tracks finished. On restart, `recover_interrupted()` re-queues pending ones, and finalization runs naturally when the last one completes.
   - Remove `CollectionState`, `TrackEntry`, and the `collections` `Mutex<HashMap>` from the runner

6. Adjust `ProgressUpdate` to also carry `collection_id` so the frontend can route SSE events to the right collection tile without maintaining a separate lookup map. (Small addition to the broadcast payload.)

**Backend — Server**

7. Update server.rs:
   - `AppState` holds `Arc<Database>` (in addition to coordinator, cover fetcher, etc.)
   - Wire new routes: `GET /api/collections`, `GET /api/collections/:id/tracks`
   - Remove `GET /api/queue`

8. Update handlers.rs:
   - `POST /api/queue` → resolve metadata via librespot, insert via `Database`, return `{ collection_id, track_ids, task_ids }`
   - `GET /api/collections` → `db.list_collections()`, serialize
   - `GET /api/collections/:id/tracks` → `db.get_tracks_for_collection(id)`, serialize
   - SSE endpoint unchanged (broadcast channel, delta events)

**Frontend — Types & API**

9. Update types.ts:
   - `CollectionItem`: `{ id, uri, type, title, artists, cover_id, date, total_tracks, status, progress, registered_at }`
   - `TrackItem`: `{ id, uri, title, artists, number, disc_number, duration_ms, task_id, status, message }` — nullable fields for skeleton rows
   - `SseEvent`: add `collection_id` field (from step 6)
   - Remove `QueueResponse`, `QueueItem`

10. Rewrite api.ts:
    - `fetchCollections()` → `GET /api/collections`
    - `fetchTracks(collectionId)` → `GET /api/collections/{id}/tracks`
    - `queueUrl(url)` → `POST /api/queue`
    - Remove `responseToQueueItem()`, status derivation logic

**Frontend — Page & Components**

11. Rewrite SSE handling in +page.svelte:
    - On SSE event: use `collection_id` from the event to find the matching `CollectionItem` in the reactive array
    - **Patch in-place**: update that collection's `status`/`progress` (re-derive from known track states, or optimistically update)
    - If the collection's track list is currently expanded & cached, patch the specific `TrackItem` by `task_id`
    - If `track_info` present on event → update skeleton row with real metadata (no re-fetch needed)
    - **No full refresh ever**. UI state (expanded cards, scroll, input focus) fully preserved.

12. Update QueueView.svelte:
    - Prop renamed: `queue` → `collections`
    - On expand: `fetchTracks(collectionId)`, cache in `Map<string, TrackItem[]>`
    - On collapse: keep cache (re-expand is instant)
    - Track data for expanded collections updated reactively via SSE patches from parent

13. Update TrackList.svelte:
    - Skeleton rows: when `title` is null, show track URI + pulsing placeholder for title/duration
    - Status dot + message rendered as before, but reactively patched per-track

14. Update mock.ts to match new API shapes. Remove stale `spotify_id`/`popularity` fields.

**Cleanup**

15. Delete queue.sled directory handling from main.rs. Log a one-time info message if queue.sled exists on disk ("Legacy queue.sled found — it is no longer used and can be deleted.").

16. Update lib.rs to export `db` module instead of `registry`.

17. Update stale docs: queue.md (already stale, rewrite to reflect new architecture).

---

### Verification

- **Unit tests** (`db.rs`): in-memory SQLite (`:memory:`) — insert/update/list/dedup/recovery/finalization-check. Test the multi-collection track edge case (same URI in two collections → two separate track rows + tasks).
- **Integration test**: Coordinator + Database + mock runner — enqueue collection, simulate run, verify status aggregation, verify finalization triggers on last track.
- **Frontend smoke test**: Load → collections render → expand → tracks with skeletons → SSE event → in-place update, no state loss. Verify expanded card *stays expanded* across updates.
- **Crash recovery test**: Insert tasks, set some to `running`, call `recover_interrupted()`, verify they're back to `pending`.

---

### Decisions

- **rusqlite (bundled)** — traditional, synchronous, zero system deps
- **REST + targeted SSE** — real-time without state-clobbering
- **`collection_id` in SSE payload** — removes need for client-side `task_id → collection_id` bookkeeping
- **Stateless runner finalization via DB** — crash-safe, no in-memory `CollectionState`
- **Cover art unchanged** — defer to future pass
- **Clean break** — no sled migration
- **Keep all history** — no pruning
- **Skeleton rows** for unresolved tracks
- **Multi-collection tracks deferred** — schema supports it (`UNIQUE(uri, collection_id)`), download dedup optimization is future work



## IMPLEMENTATION


User: Store the design doc as we agreed on and start implementation.

GitHub Copilot: Created 14 todos

Created [](file:///home/user/halfdane/fetching/docs/redesign-relational-db.md)

Starting: *Add rusqlite, remove sled dep* (2/14)

Now let me read the current state of the files I need to modify first. 

Read [](file:///home/user/halfdane/fetching/core/Cargo.toml)

Read [](file:///home/user/halfdane/fetching/core/src/lib.rs)

Now let me read the full files I'll need to understand for the db.rs implementation — registry.rs, coordinator.rs, runner.rs, container.rs, and queue.rs: 

Read [](file:///home/user/halfdane/fetching/core/src/registry.rs#1-1), lines 1 to 250

Read [](file:///home/user/halfdane/fetching/core/src/registry.rs#250-250), lines 250 to 553

Read [](file:///home/user/halfdane/fetching/core/src/coordinator.rs#1-1), lines 1 to 300

Read [](file:///home/user/halfdane/fetching/core/src/coordinator.rs#300-300), lines 300 to 615

Now let me read the remaining key files in parallel: 

Read [](file:///home/user/halfdane/fetching/core/src/runner.rs)

Read [](file:///home/user/halfdane/fetching/core/src/queue.rs)

Now let me read the container types (Track, TrackCollection), server files, and main.rs:  

Read [](file:///home/user/halfdane/fetching/core/src/container.rs#1-1), lines 1 to 150

Read [](file:///home/user/halfdane/fetching/server/src/handlers.rs)

Read [](file:///home/user/halfdane/fetching/server/src/server.rs#1-1), lines 1 to 250

Read [](file:///home/user/halfdane/fetching/server/src/server.rs#250-250), lines 250 to 266

Read [](file:///home/user/halfdane/fetching/main.rs#1-1), lines 1 to 200

Read [](file:///home/user/halfdane/fetching/main.rs#200-200), lines 200 to 221

Now let me read the remaining key files: 

Read [](file:///home/user/halfdane/fetching/core/src/queue_memory.rs#1-1), lines 1 to 50

Read [](file:///home/user/halfdane/fetching/core/src/playlist.rs#1-1), lines 1 to 60

Good, I now have full context on all the key files. Let me also check the server's Cargo.toml and the spotify_api trait: 

Read [](file:///home/user/halfdane/fetching/server/Cargo.toml)

Read [](file:///home/user/halfdane/fetching/core/src/spotify_api.rs#1-1), lines 1 to 80

Good. I have full context now. Let me start implementing step by step.

**Step 2: Update dependencies** — swap sled for rusqlite in Cargo.toml. 

Replacing 5 lines with 5 lines in [](file:///home/user/halfdane/fetching/core/Cargo.toml)


```

```


Starting: *Create core/src/db.rs module* (3/14)

Now creating the core db.rs module — the heart of this redesign. 

Created [](file:///home/user/halfdane/fetching/core/src/db.rs)

Starting: *Update coordinator for DB* (4/14)

Now updating the coordinator to use `Database` instead of `TaskRegistry`. This is a significant rewrite — replacing the registry trait with direct `Arc<Database>`, adding `collection_id` to `ProgressUpdate`, and removing `snapshot()`.  

Replacing 20 lines with 20 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the `ProgressUpdate` struct to include `collection_id`: 

Replacing 14 lines with 17 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the `DownloadCoordinator` struct and its constructors:  

Replacing 65 lines with 65 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update `add_collection` to use the database:  

Replacing 24 lines with 59 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the remaining public methods — remove `snapshot()`, update docs, and update the `enqueue` method:   

Replacing 40 lines with 35 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the worker loop — the `emit_update` calls need to include `collection_id`, and persistence goes to DB instead of registry:   

Replacing 47 lines with 61 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the result handling and `emit_update` to include `collection_id`:   

Replacing 35 lines with 38 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the `emit_update` function to use `Database` instead of `TaskRegistry`:    

Replacing 18 lines with 22 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the `JobRunner` trait to accept `collection_id`:   

Replacing 21 lines with 22 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now I need to update the tests in coordinator.rs. Let me read the test section:   

Read [](file:///home/user/halfdane/fetching/core/src/coordinator.rs#355-355), lines 355 to 615

I can see the tests need updating — the `JobRunner` trait signature changed (added `collection_id`), and `add_collection` return type changed. Let me update all the test code:  

Replacing 33 lines with 33 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```
Now update the remaining tests that use `add_collection` and `ids`:    

Read [](file:///home/user/halfdane/fetching/core/src/coordinator.rs#530-530), lines 530 to 670

I need to update all the test functions that reference `add_collection` and the old return type. Let me do a bulk replacement of the test section:   

Replacing 123 lines with 126 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```


Created 14 todos

Now the big one — rewriting runner.rs to use the DB for finalization instead of in-memory `CollectionState`. The runner needs to accept `collection_id`, use DB to update track metadata, check collection completion, and drop the `tracker` HashMap.    

Replacing 94 lines with 66 lines in [](file:///home/user/halfdane/fetching/core/src/runner.rs)


```

```
Now replace the `JobRunner` impl and the helper methods:     

Replacing 135 lines with 128 lines in [](file:///home/user/halfdane/fetching/core/src/runner.rs)


```

```
Now replace the old `record_track` / `finalise_collection` helpers with the new DB-based approach:      

Replacing 163 lines with 202 lines in [](file:///home/user/halfdane/fetching/core/src/runner.rs)


```

```


Starting: *Update core/src/lib.rs exports* (6/14)

Now update lib.rs to export db module and remove registry:       

Replacing 13 lines with 13 lines in [](file:///home/user/halfdane/fetching/core/src/lib.rs)


```

```


Starting: *Update server routes & handlers* (7/14)

Now rewrite the server — server.rs needs new routes and `AppState` with `Arc<Database>`:        

Read [](file:///home/user/halfdane/fetching/server/src/lib.rs)

Now rewrite server.rs completely — the most impactful change:          

Replacing 265 lines with 168 lines in [](file:///home/user/halfdane/fetching/server/src/server.rs)


```

```
Summarized conversation historyNow let me read the files that still need updating.   

Read [](file:///home/user/halfdane/fetching/server/src/handlers.rs)

Read [](file:///home/user/halfdane/fetching/main.rs#1-1), lines 1 to 200

Read [](file:///home/user/halfdane/fetching/server/Cargo.toml)

Read [](file:///home/user/halfdane/fetching/main.rs#200-200), lines 200 to 221

Starting: *Update main.rs* (8/14)

Now updating main.rs — replacing sled registry with Database, and adapting the batch mode for the new `add_collection` return type: 

Replacing 19 lines with 19 lines in [](file:///home/user/halfdane/fetching/main.rs)

Continue to iterate?
[object Object]

Made changes.


## CONTINUE IMPLEMENTATION?

User: @agent Continue: "Continue to iterate?"

GitHub Copilot: 
```

```
Now update `build_apis` to accept a `Database` and create `DownloadRunner::with_db`: 

Replacing 13 lines with 18 lines in [](file:///home/user/halfdane/fetching/main.rs)


```

```
Now update the Batch subcommand handler: 

Replacing 32 lines with 33 lines in [](file:///home/user/halfdane/fetching/main.rs)


```

```
Now update the Server subcommand — replace sled with Database: 

Replacing 30 lines with 41 lines in [](file:///home/user/halfdane/fetching/main.rs)


```

```


Starting: *Delete registry.rs* (9/14)

Ran terminal command: rm /home/user/halfdane/fetching/core/src/registry.rs

Now let me read the frontend files to update them.   

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/types.ts)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/api.ts)

Read [](file:///home/user/halfdane/fetching/frontend/src/routes/%2Bpage.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/QueueView.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/TrackList.svelte)

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/mock.ts#1-1), lines 1 to 200

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/AddToQueue.svelte#1-1), lines 1 to 50

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/mock.ts#200-200), lines 200 to 267

Now for the frontend. I'll update all the files to match the new REST API shape. 

Starting: *Update frontend types.ts* (10/14)

Replacing 85 lines with 98 lines in [](file:///home/user/halfdane/fetching/frontend/src/lib/types.ts)


```

```


Starting: *Update frontend api.ts* (11/14)

Now rewriting api.ts — new REST endpoints, conversion helpers, and targeted SSE handling: 

Replacing 117 lines with 140 lines in [](file:///home/user/halfdane/fetching/frontend/src/lib/api.ts)


```

```
Now update +page.svelte — replace the old full-refresh-on-SSE pattern with targeted in-place patching:  

Replacing 123 lines with 204 lines in [](file:///home/user/halfdane/fetching/frontend/src/routes/%2Bpage.svelte)


```

```
Now update the AddToQueue.svelte — it needs to pass `collection_id` instead of a `QueueItem`:  

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/AddToQueue.svelte)

Replacing 25 lines with 24 lines in [](file:///home/user/halfdane/fetching/frontend/src/lib/AddToQueue.svelte)


```

```
Now update QueueView.svelte — the `item.tracks` is no longer optional and retry uses the collection URI:   

Replacing 5 lines with 5 lines in [](file:///home/user/halfdane/fetching/frontend/src/lib/QueueView.svelte)


```

```
Now update the track list expand condition (tracks is no longer optional):    

Replacing 1 lines with 1 lines in [](file:///home/user/halfdane/fetching/frontend/src/lib/QueueView.svelte)


```

```
Now update mock.ts — add the new mock functions and update mock data to match the new types:  

Replacing 119 lines with 124 lines in [](file:///home/user/halfdane/fetching/frontend/src/lib/mock.ts)


```

```
Now update the mock functions at the bottom of the file:  

Replacing 77 lines with 67 lines in [](file:///home/user/halfdane/fetching/frontend/src/lib/mock.ts)


```

```
Now let me also check the `index.ts` barrel file to see if it re-exports anything that changed: 

Read [](file:///home/user/halfdane/fetching/frontend/src/lib/index.ts)

Good, it's empty. Now let me read the Rust instructions file, then attempt to build:   

Read [](file:///home/user/global-instructions/rust.instructions.md)

Starting: *Build & fix compile errors* (13/14)

Now let's try to build and fix compile errors:    

Ran terminal command: cargo build 2>&1 | head -150

Two errors in main.rs. Let me check the types: 

Searched for regex `QueueEntry|RecoveredEntry` (`**/core/src/**`), 20 results

Read [](file:///home/user/halfdane/fetching/core/src/db.rs#112-112), lines 112 to 140

Read [](file:///home/user/halfdane/fetching/core/src/queue.rs#1-1), lines 1 to 50

I see the issues. `RecoveredEntry` has a `TrackCollection` (not `Arc<TrackCollection>`) while `QueueEntry` expects `Arc<TrackCollection>`. I need to either:
1. Make `enqueue()` accept `RecoveredEntry` 
2. Add a conversion from `RecoveredEntry` to `QueueEntry`

The cleanest approach: add a `From<RecoveredEntry> for QueueEntry` impl in the db module. Also need to fix the `task_id` type mismatch in main.rs batch mode. 

Read [](file:///home/user/halfdane/fetching/core/src/db.rs#620-620), lines 620 to 680

Searched for text `fn get_collection` (`**/core/src/db.rs`), 2 results

`get_collection` returns `TrackCollection` (not `Arc`), so `RecoveredEntry.collection` is `TrackCollection`. The `QueueEntry` needs `Arc<TrackCollection>`. Let me add a conversion and fix main.rs: 

Read [](file:///home/user/halfdane/fetching/core/src/db.rs#115-115), lines 115 to 127

I need to add `impl From<RecoveredEntry> for QueueEntry` so that `enqueue()` can accept recovered entries. Let me add it after the RecoveredEntry struct:  

Replacing 7 lines with 17 lines in [](file:///home/user/halfdane/fetching/core/src/db.rs)


```

```
Now fix main.rs — use `.into()` for the recovery loop, and fix the task_id type in batch mode: 

Replacing 3 lines with 3 lines in [](file:///home/user/halfdane/fetching/main.rs)


```

```
Now fix the batch mode `task_id` comparison — `update.task_id` is `Uuid` but `all_task_ids` is `HashSet<String>`:    

Replacing 1 lines with 1 lines in [](file:///home/user/halfdane/fetching/main.rs)


```

```


Ran terminal command: cargo build 2>&1 | head -80

Build succeeds. Let me clean up the warnings:    

Replacing 8 lines with 5 lines in [](file:///home/user/halfdane/fetching/server/src/server.rs), [](file:///home/user/halfdane/fetching/core/src/coordinator.rs), [](file:///home/user/halfdane/fetching/core/src/runner.rs)


```

```

```

```


Searched for text `collection_id: &str,` (`**/core/src/coordinator.rs`), 2 results

Read [](file:///home/user/halfdane/fetching/core/src/coordinator.rs#208-208), lines 208 to 218

Replacing 6 lines with 6 lines in [](file:///home/user/halfdane/fetching/core/src/coordinator.rs)


```

```


Ran terminal command: cargo build 2>&1 | head -40

Clean build — zero warnings, zero errors. Now run tests:    

Ran terminal command: cargo test 2>&1 | tail -60

Ran terminal command: cargo test --workspace 2>&1 | tail -80

**80 tests passed, 0 failed.** Clean build, all green. Now let me update queue.md:     

Read [](file:///home/user/halfdane/fetching/docs/queue.md#1-1), lines 1 to 100

Read [](file:///home/user/halfdane/fetching/docs/queue.md#100-100), lines 100 to 127

Replacing 126 lines with 128 lines in [](file:///home/user/halfdane/fetching/docs/queue.md)


```

```


Completed: *Build & fix compile errors* (13/14)

🍀 All 14 steps complete. Here's what was done:

**Backend (Rust):**
- Replaced `sled` with `rusqlite` (bundled SQLite) in Cargo.toml
- Created db.rs — full `Database` module with normalized schema (collections, tracks, tasks), all CRUD methods, recovery, and 16 tests
- Rewrote coordinator.rs — `Database` replaces `TaskRegistry`, `ProgressUpdate` gains `collection_id`, `add_collection` returns `(collection_id, Vec<(track_id, task_id)>)`
- Rewrote runner.rs — stateless finalization via `db.is_collection_complete()` instead of in-memory state tracking
- Rewrote server.rs — new `GET /api/collections` + `GET /api/collections/{id}/tracks` endpoints, removed old `GET /api/queue` + `QueueResponse`
- Updated main.rs — `Database::open("fetching.db")` replaces `SledRegistry::open("queue.sled")`, both Batch and Server modes adapted
- Deleted `core/src/registry.rs`

**Frontend (Svelte/TS):**
- Rewrote types.ts — new `CollectionRow`, `TrackRow`, `PostQueueResponse` server types; `QueueItem`/`TrackItem` updated with `task_id` and `uri` fields
- Rewrote api.ts — `fetchCollections()` + `fetchCollectionTracks()` replace `fetchQueue()`, removed `subscribeSseSignal` (replaced by targeted `subscribeEvents`)
- Rewrote +page.svelte — in-place SSE patching via `collection_id`/`task_id` instead of debounced full-refresh
- Updated AddToQueue.svelte, QueueView.svelte, mock.ts

**Build result:** Clean compile (0 warnings), **80 tests passing**.

Made changes.