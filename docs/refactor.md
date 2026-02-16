# Web GUI Implementation Plan

**Project:** `fetching` (Rust-based Spotify downloader with CLI and web interface)

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





## Example Flow

1. User submits album URL → Task created, progress updates sent via SSE.
2. Track 3 fails → SSE sends `{"task_id": "...", "status": "Failed: Download error", "current": 3, "total": 10}`
3. GUI shows "Retry" button for the album.
4. User clicks retry → POST `/api/queue` with same URL → New task created.
5. Process repeats, hopefully succeeding this time.

This keeps the architecture simple: SSE for updates, HTTP for actions. No need for WebSockets or bidirectional communication. The retry just re-queues the URL, leveraging the existing queue system.




