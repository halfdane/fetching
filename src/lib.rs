// Library interface for integration tests

pub mod auth;
pub mod cache;
pub mod cli;
pub mod config;
pub mod error;
pub mod implementations;
pub mod input;
pub mod metadata;
pub mod mocks;
pub mod m3u;
pub mod playback;
pub mod processor;
pub mod stream;
pub mod traits;

// Re-export mocks for external test access
pub use mocks::*;

// Re-export create_session for main.rs
pub use crate::auth::session::create_session;
