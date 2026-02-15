# Web GUI Implementation Plan

**Project:** `spotify-player` (Rust-based Spotify downloader with CLI and web interface)

**Goal:** Build a web application with real-time progress updates, using the existing core library.

## Overall Architecture

- **`core` library crate**: Existing Spotify logic (auth, URL processing, downloads, progress reporting).
- **`server` binary crate**: Axum HTTP server exposing REST API and SSE for progress.
- **Frontend**: Simple static HTML/JS served by Axum, with live updates via SSE.

**Key Technologies:**
- **Axum**: Modern Rust web framework (async, Tokio-based).
- **SSE (Server-Sent Events)**: For real-time progress updates (simpler than WebSockets for this use case).
- **Workspace**: Clean separation between core logic and web server.

## Testing Guidelines

**Current test count:** 79+ unit tests + integration tests.

- Run `cargo test -q` after each major change to ensure nothing breaks.
- Run `cargo test` for full output if needed.
- Never delete tests; move them with refactored code.
- Use `cargo test -- --list` to verify all tests are discovered.

## Implementation Steps

### 1. Rename Workspace Member (src → core)

**Current:** Workspace members = ["src"]  
**Target:** members = ["core", "server"]

- Rename `src/` directory to `core/`.
- Update root `Cargo.toml`: `members = ["core", "server"]`
- Update `core/Cargo.toml` paths if needed (should be relative).
- Run `cargo check` to verify.

### 2. Enhance Progress Reporting in Core

**Current:** `ProgressUpdate` struct exists with basic fields.  
**Enhance to:**
```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressUpdate {
    pub task_id: uuid::Uuid,
    pub scope: ProgressScope,       // Track, Album, Playlist, Global
    pub status: String,             // Human-readable status
    pub current: u32,               // current item index
    pub total: u32,                 // total items, if known
    pub item: String,               // track/album name
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum ProgressScope {
    Track,
    Album,
    Playlist,
    Global,
}
```

- Update `core/src/lib.rs` to include `scope` and `current/total` fields.
- Ensure `serde` features include `derive`.
- Update existing `tx.send()` calls to use new fields.

### 3. Update Core API Signature

**Current:** `process_uris(uris: &[String], tx: Sender<ProgressUpdate>)`  
**Update to:** Support task_id per URL.

- Modify `process_uris` to handle multiple URLs with individual task_ids.
- Or create `process_url(task_id: Uuid, url: String, tx: Sender<ProgressUpdate>)` and call it per URL.

### 4. Create Server Crate

- `cargo new server --bin`
- Add to workspace members: `["core", "server"]`
- In `server/Cargo.toml`:
  ```toml
  [dependencies]
  axum = "0.7"
  tokio = { version = "1", features = ["full"] }
  serde = { version = "1", features = ["derive"] }
  serde_json = "1"
  tower = "0.4"
  tower-http = { version = "0.5", features = ["fs"] }
  uuid = { version = "1", features = ["v4"] }
  spotify-player-core = { path = "../core" }
  ```

### 5. Implement Axum Server Skeleton

**AppState:**
```rust
#[derive(Clone)]
struct AppState {
    task_tx: mpsc::Sender<Task>,
    progress_tx: broadcast::Sender<ProgressUpdate>,
    auth_token: String,
}
```

**Routes:**
- `GET /`: Serve static HTML
- `POST /api/queue`: Queue URL
- `GET /api/status`: Get task statuses
- `GET /events`: SSE stream

**Worker Loop:**
- Receive tasks from mpsc channel
- Call `core::process_url()` with progress forwarding
- Forward progress updates to broadcast channel

### 6. Add Authentication

- Require `X-Auth-Token` header for `/api/*` and `/events`
- Read token from env var at startup
- Return 401 for invalid/missing tokens

### 7. Implement Frontend

**Static files:** `server/static/index.html`
- Form to submit Spotify URLs
- Table for task status
- JavaScript EventSource for SSE updates

### 7a. Testing on Dev Machine

- run server locally
- make user download tracks, albums, playlists
- ask him about every feature on its own, he'll forget them otherwise!

### 8. Deployment Considerations

- For ada (NixOS): Package as systemd service
- Use agenix for auth token secrets
- Bind to 0.0.0.0:8080

## Key Choices & Rationale

- **Axum over Actix/Rocket**: Modern, excellent Tokio integration, strong community.
- **SSE over WebSockets**: Sufficient for server→client progress; simpler implementation.
- **Broadcast channels**: Efficient fan-out of progress to multiple clients.
- **Static HTML/JS**: Lightweight, no build step required.

## Validation Steps

- After each step: `cargo check`, `cargo test -q`
- Test web server: `cargo run --bin server`, access http://localhost:8080
- Verify progress updates in browser console

This plan builds on the existing progress infrastructure and provides a scalable web interface.



# MUCH LATER: Implement retry 

1. **Enhance Progress Updates for Failures**:
   - Send `ProgressUpdate` with `status: "Failed: <error>"` when individual tracks fail.
   - For total failures, mark the task as failed in the server's state.

2. **Update Status Endpoint**:
   - Modify `GET /api/status` to return failure details per task/URL.
   - Include fields like `failed_tracks: Vec<String>`, `error_message: Option<String>`.

3. **GUI Retry Button**:
   - Display failed tasks with a "Retry" button.
   - On click, POST to `/api/queue` with the original URL.
   - The server treats it as a new task (new `task_id`).

4. **Server-Side Handling**:
   - No changes needed to the queue/worker loop - it already processes tasks sequentially.
   - Optionally, add deduplication if you want to prevent duplicate concurrent retries.

## Example Flow

1. User submits album URL → Task created, progress updates sent via SSE.
2. Track 3 fails → SSE sends `{"task_id": "...", "status": "Failed: Download error", "current": 3, "total": 10}`
3. GUI shows "Retry" button for the album.
4. User clicks retry → POST `/api/queue` with same URL → New task created.
5. Process repeats, hopefully succeeding this time.

This keeps the architecture simple: SSE for updates, HTTP for actions. No need for WebSockets or bidirectional communication. The retry just re-queues the URL, leveraging the existing queue system.





- [ ] 1. Verify server/Cargo.toml dependencies (axum, tokio, etc.) and add any missing ones
- [ ] 2. Add hyper = "1" to server/Cargo.toml for server binding
- [ ] 3. Create/clean up server/src/main.rs: remove all code except a minimal async main that prints "Hello, world!"
- [ ] 4. Add a minimal Axum app with a single GET / route returning "OK"
- [ ] 5. Replace axum::Server with the correct hyper/axum serve pattern for Axum 0.7+
- [ ] 6. Add AppState struct and wire it into the Axum app (no logic yet)
- [ ] 7. Add Task struct and channel definitions (no logic yet)
- [ ] 8. Add stubs for all required routes: GET /, POST /api/queue, GET /api/status, GET /events
- [ ] 9. Ensure the server builds and runs after each step