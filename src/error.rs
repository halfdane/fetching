//! Custom error types for streaming and caching operations.
//!
//! Provides structured error variants for common failure modes like
//! missing HOME directory, invalid UTF-8 paths, and network failures.
//! Uses `thiserror` for ergonomic error handling.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum DownloadError {
    #[error("HOME environment variable not set")]
    HomeNotSet,

    #[error("Path contains invalid UTF-8: {0:?}")]
    InvalidUtf8Path(PathBuf),

    #[error("Invalid Spotify URI: {0}")]
    InvalidUri(String),

    #[error("Failed to stream track: {0}")]
    DownloadFailed(String),

    #[error("Failed to write metadata: {0}")]
    MetadataWriteFailed(String),

    #[error("Failed to create directory: {path:?}")]
    DirectoryCreationFailed { path: PathBuf },

    #[error("Failed to move file from {from:?} to {to:?}")]
    FileMoveFailed { from: PathBuf, to: PathBuf },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Librespot error: {0}")]
    Librespot(#[from] librespot_core::Error),

    #[error("Image processing error: {0}")]
    Image(#[from] image::ImageError),
}

pub type Result<T> = std::result::Result<T, DownloadError>;
