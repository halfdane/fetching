//! Trait abstractions for testability.
//!
//! Defines various traits to decouple business logic from
//! librespot implementation, enabling mocking in tests.

pub mod audio;
pub mod metadata;
pub mod fetchers;

// Re-exports for backward compatibility
pub use audio::*;
pub use metadata::*;
pub use fetchers::*;