//! Workspace persistence traits.
//!
//! Defines the dyn-safe [`WorkspaceStore`] and its native-async sibling
//! [`NativeWorkspaceStore`] for workspace documents, chunks, and semantic
//! search.

use core::future::Future;

use uuid::Uuid;

use crate::db::params::DbFuture;
use crate::error::WorkspaceError;
use crate::workspace::{MemoryChunk, MemoryDocument, SearchConfig, SearchResult, WorkspaceEntry};

/// Parameters for `insert_chunk`.
pub struct InsertChunkParams<'a> {
    /// Durable UUID of the parent document that will own this chunk.
    pub document_id: Uuid,
    /// Zero-based ordinal of this chunk within the document.
    pub chunk_index: u32,
    /// UTF-8 chunk body to persist for search and retrieval.
    pub content: &'a str,
    /// Optional embedding vector for the chunk; when present it should match
    /// the backend's expected floating-point dimensionality.
    pub embedding: Option<&'a [f32]>,
}

/// Parameters for `hybrid_search`.
pub struct HybridSearchParams<'a> {
    /// Owning user identifier used to scope the search query.
    pub user_id: &'a str,
    /// Optional agent UUID for agent-scoped workspace searches.
    pub agent_id: Option<Uuid>,
    /// UTF-8 search query text used for lexical ranking and diagnostics.
    pub query: &'a str,
    /// Optional query embedding; when present it should use the same vector
    /// shape as stored chunk embeddings.
    pub embedding: Option<&'a [f32]>,
    /// Search controls such as limits, weighting, and FTS/vector tuning.
    pub config: &'a SearchConfig,
}

/// Object-safe persistence surface for workspace documents, chunks, and
/// semantic search.
///
/// This trait provides the dyn-safe boundary for workspace storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn WorkspaceStore>`).  It uses
/// boxed futures ([`DbFuture`]) to maintain object safety.
///
/// Companion trait: [`NativeWorkspaceStore`] provides the same API using
/// native async traits (RPITIT).  A blanket adapter automatically bridges
/// implementations of `NativeWorkspaceStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support
/// concurrent access.
pub trait WorkspaceStore: Send + Sync {
    /// Load one document by logical path.
    ///
    /// Returns `WorkspaceError` when the document is missing or the lookup
    /// fails.
    fn get_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    /// Load one document by its durable ID.
    fn get_document_by_id<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    /// Load or create a document at `path` for the given owner scope.
    ///
    /// Missing documents are created with empty/default content.
    fn get_or_create_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    /// Replace the stored content for an existing document ID.
    fn update_document<'a>(
        &'a self,
        id: Uuid,
        content: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    /// Delete the document addressed by path within the given owner scope.
    ///
    /// Missing paths are reported through `WorkspaceError`.
    fn delete_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    /// List the immediate entries within `directory`.
    fn list_directory<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        directory: &'a str,
    ) -> DbFuture<'a, Result<Vec<WorkspaceEntry>, WorkspaceError>>;
    /// List every stored path for the given owner scope.
    fn list_all_paths<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<String>, WorkspaceError>>;
    /// List all stored documents for the given owner scope.
    fn list_documents<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<MemoryDocument>, WorkspaceError>>;
    /// Delete all chunks belonging to `document_id`.
    fn delete_chunks<'a>(&'a self, document_id: Uuid) -> DbFuture<'a, Result<(), WorkspaceError>>;
    /// Insert one chunk and return its new chunk ID.
    fn insert_chunk<'a>(
        &'a self,
        params: InsertChunkParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, WorkspaceError>>;
    /// Persist an embedding for an existing chunk.
    fn update_chunk_embedding<'a>(
        &'a self,
        chunk_id: Uuid,
        embedding: &'a [f32],
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    /// Return chunks that still need embeddings.
    ///
    /// `limit` caps the number of returned chunk records.
    fn get_chunks_without_embeddings<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> DbFuture<'a, Result<Vec<MemoryChunk>, WorkspaceError>>;
    /// Execute the backend's hybrid search for the supplied parameters.
    ///
    /// Returns ranked results or `WorkspaceError` when retrieval fails.
    fn hybrid_search<'a>(
        &'a self,
        params: HybridSearchParams<'a>,
    ) -> DbFuture<'a, Result<Vec<SearchResult>, WorkspaceError>>;
}

/// Native async sibling trait for concrete workspace-store implementations.
pub trait NativeWorkspaceStore: Send + Sync {
    /// Load one document by logical path.
    fn get_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    /// Load one document by its durable ID.
    fn get_document_by_id<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    /// Load or create a document at `path` for the given owner scope.
    fn get_or_create_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    /// Replace the stored content for an existing document ID.
    fn update_document<'a>(
        &'a self,
        id: Uuid,
        content: &'a str,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    /// Delete the document addressed by path within the given owner scope.
    fn delete_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    /// List the immediate entries within `directory`.
    fn list_directory<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        directory: &'a str,
    ) -> impl Future<Output = Result<Vec<WorkspaceEntry>, WorkspaceError>> + Send + 'a;
    /// List every stored path for the given owner scope.
    fn list_all_paths<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<String>, WorkspaceError>> + Send + 'a;
    /// List all stored documents for the given owner scope.
    fn list_documents<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<MemoryDocument>, WorkspaceError>> + Send + 'a;
    /// Delete all chunks belonging to `document_id`.
    fn delete_chunks<'a>(
        &'a self,
        document_id: Uuid,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    /// Insert one chunk and return its new chunk ID.
    fn insert_chunk<'a>(
        &'a self,
        params: InsertChunkParams<'a>,
    ) -> impl Future<Output = Result<Uuid, WorkspaceError>> + Send + 'a;
    /// Persist an embedding for an existing chunk.
    fn update_chunk_embedding<'a>(
        &'a self,
        chunk_id: Uuid,
        embedding: &'a [f32],
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    /// Return chunks that still need embeddings.
    fn get_chunks_without_embeddings<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<MemoryChunk>, WorkspaceError>> + Send + 'a;
    /// Execute the backend's hybrid search for the supplied parameters.
    fn hybrid_search<'a>(
        &'a self,
        params: HybridSearchParams<'a>,
    ) -> impl Future<Output = Result<Vec<SearchResult>, WorkspaceError>> + Send + 'a;
}
