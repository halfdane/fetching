# Web GUI Implementation Plan

**Project:** `fetching` (Rust-based Spotify downloader with CLI and web interface)

## Overall Architecture

- **`core` library crate**: Existing Spotify logic (auth, URL processing, downloads, progress reporting).
- **`server` binary crate**: Axum HTTP server exposing REST API and SSE for progress.
- **Frontend**: Simple static HTML/JS served by Axum, with live updates via SSE.

**Key Technologies:**

- **Axum**: Modern Rust web framework (async, Tokio-based).
- **SSE (Server-Sent Events)**: For real-time progress updates (simpler than WebSockets for this use case).
- **Workspace**: Clean separation between core logic and web server.

## Testing Guidelines

- Run `cargo test -q` after each major change to ensure nothing breaks.
- Run `cargo test` for full output if needed.
- if production code is dead, it can be removed together with the corresponding test, after user's verification
- Othwerwise: never delete tests; move them with refactored code
- Use `cargo test -- --list` to verify all tests are discovered.

## Implementation Steps







