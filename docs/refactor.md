# ⚠️ Strict Adherence Required

**The plan below is part of an intricate multi-step refactoring. You must strictly adhere to every step and instruction. Method signatures and code structure *must* be accurate and not deviate from what is specified.**

# Refactoring Plan: Moving Core Logic to lib.rs

This guide will help you refactor the project to move core logic from `main.rs` to `lib.rs`, making the codebase more modular and testable. Each step is explained for a novice Rust developer.


## 0. Test Inventory and Preservation

**Current test count:**  
There are around 207 unit and integration tests in the codebase, found in both src/ and tests/ directories. These include tests using #[test] and #[tokio::test] attributes.

**Test locations:**  
- Integration tests: tests/integration/, tests/cache_integration_tests.rs, tests/integration/test_temp_file_cleanup.rs, etc.
- Unit tests: Scattered throughout modules such as src/stream.rs, src/auth/token.rs, src/auth/session.rs, src/auth/oauth.rs, src/auth/mod.rs, src/input.rs, src/cli/mod.rs, src/metadata/builders.rs, src/m3u.rs, src/metadata/validation.rs, src/metadata/tags.rs, and more.

**Preservation steps:**  
- When moving production code (functions, modules, structs) from one file to another, always move the corresponding mod tests { ... } or any #[cfg(test)] blocks along with it.
- If a test covers code that is being split across modules, move the test to the module that now contains the logic it covers, or refactor the test to import the new location.
- After each move, run `cargo test` to ensure all tests are still present and passing.
- If a test fails to compile due to a missing import or path, update the test to use the new path, but no other changes to the test logic are allowed!
- Never delete a test unless you are certain it is obsolete and covered elsewhere.

**Tip:**  
If you are unsure whether a test is still being run, use `cargo test -- --list` to see all discovered tests.


## 1. Overview
- Move Spotify authentication, URL processing, and API logic from `main.rs` to `lib.rs`.
- Expose these as public functions/modules in `lib.rs`.
- Refactor `main.rs` to use the new library interface.
- Run `cargo test` after each major change to ensure nothing breaks.

## 2. Preparation
- Ensure your working directory is clean (no uncommitted changes).
- Run `cargo test` to confirm the current state is working.

## 3. Create/Update `lib.rs`
- If not present, create `src/lib.rs`.
- Add public module declarations for all core components:
  ```rust
  pub mod auth;
  pub mod playback;
  pub mod processor;
  pub mod stream;
  pub mod config;
  pub mod error;
  pub mod input;
  pub mod m3u;
  pub mod cache;
  pub mod implementations;
  pub mod metadata;
  pub mod traits;
  ```
- Add a public async function for URL processing:
  ```rust
  use std::error::Error;
  pub async fn process_url(url: &str) -> Result<(), Box<dyn Error>> {
      // Implementation will be moved from main.rs
      Ok(())
  }
  ```
- Re-export any helpers needed by `main.rs` (e.g., authentication/session helpers):
  ```rust
  pub use crate::auth::session::{create_session_with_auto_refresh, create_authenticated_session};
  pub use crate::auth::get_credentials;
  pub use crate::auth::token::{TokenRefresher, read_token_data, save_token_data, is_token_expired};
  ```

## 4. Move Authentication Logic
- In `main.rs`, find all code related to Spotify authentication (e.g., session creation, token refresh).
- Move the logic to appropriate modules in `lib.rs` (usually `auth/session.rs` or `processor.rs`).
- If a function is only used for authentication/session, move it to `auth/session.rs`.
- If a function is a general helper, move it to a relevant module and make it `pub` if needed.
- Remove the moved code from `main.rs`.
- Update `lib.rs` to re-export these functions if `main.rs` or tests need them.
- Run `cargo test` to ensure everything still works.

## 5. Move process_url and Related Logic
- Identify the `process_url` function and any helpers it uses in `main.rs`.
- Move these to `lib.rs` (or a submodule if appropriate).
- Make sure all dependencies are imported and visible.
- Update `lib.rs` to export `process_url` as `pub async fn process_url(url: &str) -> Result<(), Box<dyn Error>>`.
- Run `cargo test`.

## 6. Move FETCH and API Functions
- Find any functions in `main.rs` that handle HTTP requests, Spotify API calls, or data fetching.
- Move these to `lib.rs` or a relevant submodule.
- Make them `pub` if they need to be accessed from outside.
- Run `cargo test`.

## 7. Refactor main.rs
- Remove all logic that has been moved to `lib.rs`.
- Import the new functions from `lib.rs`:
  ```rust
  use spotify_player::process_url;
  ```
- Update the main entry point to call `process_url` as needed.
- Run `cargo test`.

## 8. Final Checks
- Ensure all moved functions have correct `pub`/`async` signatures and error handling.
- Update module visibility and `use` statements as needed for compilation.
- Run `cargo test` one last time to confirm everything works.

## 9. Troubleshooting
- If you get a file not found or module not found error, check your `mod` declarations and file paths.
- If a function is not visible, ensure it is marked `pub` and the module is also `pub`.
- If tests fail, review the error messages and check for missing imports or logic.

---

This plan will help you modularize your Rust project and make it easier to test and maintain. If you get stuck, review the error messages and check the module structure.
