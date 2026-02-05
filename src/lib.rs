// Library interface for integration tests

pub mod auth;
pub mod mocks;
pub mod cache;
pub mod config;
pub mod stream;
pub mod error;
pub mod m3u;
pub mod metadata;
pub mod traits;
pub mod implementations;

// Re-export mocks for external test access
pub use mocks::*;
