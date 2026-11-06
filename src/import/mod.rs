//! OpenClaw migration and import functionality.
//!
//! Provides tools to migrate existing OpenClaw installations (memory, history,
//! settings, and credentials) into Axinite without data loss.

#[cfg(feature = "import")]
pub mod openclaw;

use std::path::PathBuf;

/// Configuration options for OpenClaw import.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Path to the OpenClaw directory (default: ~/.openclaw).
    pub openclaw_path: PathBuf,
    /// Dry-run mode: report what would be imported without writing to DB.
    pub dry_run: bool,
    /// Re-embed memory documents if dimension mismatch detected.
    pub re_embed: bool,
    /// User ID for scoping imported data.
    pub user_id: String,
}

/// Statistics collected during an import operation.
#[derive(Debug, Clone, Default)]
pub struct ImportStats {
    /// Number of workspace documents imported.
    pub documents: usize,
    /// Number of memory chunks imported.
    pub chunks: usize,
    /// Number of conversations imported.
    pub conversations: usize,
    /// Number of messages imported.
    pub messages: usize,
    /// Number of settings imported.
    pub settings: usize,
    /// Number of credentials imported.
    pub secrets: usize,
    /// Number of items skipped (already existed).
    pub skipped: usize,
    /// Number of chunks queued for re-embedding.
    pub re_embed_queued: usize,
}

impl ImportStats {
    /// Check if any items were imported.
    pub fn is_empty(&self) -> bool {
        self.documents == 0
            && self.chunks == 0
            && self.conversations == 0
            && self.messages == 0
            && self.settings == 0
            && self.secrets == 0
    }

    /// Total number of items imported.
    pub fn total_imported(&self) -> usize {
        self.documents
            + self.chunks
            + self.conversations
            + self.messages
            + self.settings
            + self.secrets
    }
}

/// Errors that can occur during import.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The OpenClaw directory or its `openclaw.json` does not exist.
    #[error("OpenClaw not found at {path}: {reason}")]
    NotFound {
        /// The directory or file that was looked up.
        path: PathBuf,
        /// Why the lookup failed, such as `Directory does not exist`.
        reason: String,
    },

    /// `openclaw.json` could not be parsed, or the import feature is not compiled in.
    #[error("JSON5 parse error: {0}")]
    ConfigParse(String),

    /// Opening, querying or decoding an OpenClaw SQLite database failed.
    #[error("SQLite error: {0}")]
    Sqlite(String),

    /// Writing to Axinite's database failed, or a chunk index was invalid.
    #[error("Database error: {0}")]
    Database(String),

    /// A workspace document error during import.
    #[error("Workspace error: {0}")]
    Workspace(String),

    /// A secrets store error while importing credentials.
    #[error("Secret error: {0}")]
    Secret(String),

    /// A filesystem error while reading OpenClaw files, converted from `std::io::Error`.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Data that was not valid UTF-8.
    #[error("Invalid UTF-8: {0}")]
    InvalidUtf8(String),
}
