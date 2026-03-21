//! Workspace and memory error types.

/// Workspace/memory errors.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// Raised when a document of `doc_type` belonging to `user_id` cannot be
    /// found in the workspace store.
    #[error("Document not found: {doc_type}")]
    DocumentNotFound { doc_type: String, user_id: String },

    /// Raised when a workspace search query fails.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Search failed: {reason}")]
    SearchFailed { reason: String },

    /// Raised when embedding generation for workspace content fails.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Embedding generation failed: {reason}")]
    EmbeddingFailed { reason: String },

    /// Raised when document chunking fails during ingestion.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Document chunking failed: {reason}")]
    ChunkingFailed { reason: String },

    /// Raised when the supplied `doc_type` is not recognised by the workspace
    /// layer.
    #[error("Invalid document type: {doc_type}")]
    InvalidDocType { doc_type: String },

    /// Raised when workspace state for `user_id` has not been initialised
    /// before use.
    #[error("Workspace not initialized")]
    NotInitialized { user_id: String },

    /// Raised when a workspace heartbeat operation fails.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Heartbeat error: {reason}")]
    HeartbeatError { reason: String },

    /// Raised for underlying I/O failures not covered by other variants.
    ///
    /// `reason` contains the underlying error detail.
    #[error("I/O error: {reason}")]
    IoError { reason: String },
}
