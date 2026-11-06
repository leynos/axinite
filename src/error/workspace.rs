//! Workspace and memory error types.

/// Workspace/memory errors.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// Raised when a document of `doc_type` belonging to `user_id` cannot be
    /// found in the workspace store.
    #[error("Document not found: {doc_type}")]
    DocumentNotFound {
        /// Document type or path that was looked up.
        doc_type: String,
        /// Owner whose workspace was searched.
        user_id: String,
    },

    /// Raised when a workspace search query fails.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Search failed: {reason}")]
    SearchFailed {
        /// Underlying error text.
        reason: String,
    },

    /// Raised when embedding generation for workspace content fails.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Embedding generation failed: {reason}")]
    EmbeddingFailed {
        /// Underlying error text.
        reason: String,
    },

    /// Raised when document chunking fails during ingestion.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Document chunking failed: {reason}")]
    ChunkingFailed {
        /// Underlying error text.
        reason: String,
    },

    /// Raised when the supplied `doc_type` is not recognized by the workspace
    /// layer.
    #[error("Invalid document type: {doc_type}")]
    InvalidDocType {
        /// The rejected document type.
        doc_type: String,
    },

    /// Raised when workspace state for `user_id` has not been initialized
    /// before use.
    #[error("Workspace not initialized")]
    NotInitialized {
        /// Owner whose workspace has no state yet.
        user_id: String,
    },

    /// Raised when a workspace heartbeat operation fails.
    ///
    /// `reason` contains the underlying error detail.
    #[error("Heartbeat error: {reason}")]
    HeartbeatError {
        /// Underlying error text.
        reason: String,
    },

    /// Raised for underlying I/O failures not covered by other variants.
    ///
    /// `reason` contains the underlying error detail.
    #[error("I/O error: {reason}")]
    IoError {
        /// Underlying error text.
        reason: String,
    },
}
