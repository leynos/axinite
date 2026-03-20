//! Workspace and memory error types.

/// Workspace/memory errors.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("Document not found: {doc_type} for user {user_id}")]
    DocumentNotFound { doc_type: String, user_id: String },

    #[error("Search failed: {reason}")]
    SearchFailed { reason: String },

    #[error("Embedding generation failed: {reason}")]
    EmbeddingFailed { reason: String },

    #[error("Document chunking failed: {reason}")]
    ChunkingFailed { reason: String },

    #[error("Invalid document type: {doc_type}")]
    InvalidDocType { doc_type: String },

    #[error("Workspace not initialized for user {user_id}")]
    NotInitialized { user_id: String },

    #[error("Heartbeat error: {reason}")]
    HeartbeatError { reason: String },

    #[error("I/O error: {reason}")]
    IoError { reason: String },
}
