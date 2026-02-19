//! Track metadata and OGG Vorbis tagging.
//!
//! This module handles conversion of Spotify track metadata to OGG Vorbis tags,
//! filename sanitization, and file path construction with proper
//! artist/album organization.

pub mod builders;
pub mod tags;
pub mod validation;

// Re-export the main functions for backward compatibility
pub use builders::build_track_path;
pub use tags::{TrackMetadata, write_ogg_tags};
pub use validation::sanitize;
