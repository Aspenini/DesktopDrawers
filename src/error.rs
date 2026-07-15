//! Explicit error type for DesktopDrawers.

use std::path::PathBuf;

/// Top-level result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("I/O error: {0}")]
    Bare(#[from] std::io::Error),

    #[error("failed to (de)serialize configuration: {0}")]
    Json(#[from] serde_json::Error),

    #[error("configuration schema version {found} is newer than supported {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },

    #[error("drawer {0} was not found")]
    DrawerNotFound(String),

    #[error("invalid command line: {0}")]
    BadCommandLine(String),

    #[error("grid is full: no free cell for a new item")]
    GridFull,

    #[error("Windows API call failed: {0}")]
    Win32(#[from] windows::core::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Attach a filesystem path to a bare I/O error for better diagnostics.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
