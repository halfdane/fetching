//! Caching operations for Spotify content.
//!
//! This module handles downloading, processing, and organizing Spotify tracks,
//! albums, and playlists into local files with metadata and playlists.

pub mod cache;
pub mod helpers;
pub mod images;
pub mod processors;

// Re-export the main functions for backward compatibility
pub use cache::*;
pub use processors::*;
