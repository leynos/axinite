//! Workspace persistence traits.
//!
//! Defines the dyn-safe [`WorkspaceStore`] and its native-async sibling
//! [`NativeWorkspaceStore`] for workspace documents, chunks, and semantic
//! search.

use core::future::Future;

use uuid::Uuid;

use crate::db::params::{DbFuture, HybridSearchParams, InsertChunkParams};
use crate::error::WorkspaceError;
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

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
    fn get_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    fn get_document_by_id<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    fn get_or_create_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<MemoryDocument, WorkspaceError>>;
    fn update_document<'a>(
        &'a self,
        id: Uuid,
        content: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn delete_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn list_directory<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        directory: &'a str,
    ) -> DbFuture<'a, Result<Vec<WorkspaceEntry>, WorkspaceError>>;
    fn list_all_paths<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<String>, WorkspaceError>>;
    fn list_documents<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> DbFuture<'a, Result<Vec<MemoryDocument>, WorkspaceError>>;
    fn delete_chunks<'a>(&'a self, document_id: Uuid) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn insert_chunk<'a>(
        &'a self,
        params: InsertChunkParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, WorkspaceError>>;
    fn update_chunk_embedding<'a>(
        &'a self,
        chunk_id: Uuid,
        embedding: &'a [f32],
    ) -> DbFuture<'a, Result<(), WorkspaceError>>;
    fn get_chunks_without_embeddings<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> DbFuture<'a, Result<Vec<MemoryChunk>, WorkspaceError>>;
    fn hybrid_search<'a>(
        &'a self,
        params: HybridSearchParams<'a>,
    ) -> DbFuture<'a, Result<Vec<SearchResult>, WorkspaceError>>;
}

/// Native async sibling trait for concrete workspace-store implementations.
pub trait NativeWorkspaceStore: Send + Sync {
    fn get_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    fn get_document_by_id<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    fn get_or_create_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<MemoryDocument, WorkspaceError>> + Send + 'a;
    fn update_document<'a>(
        &'a self,
        id: Uuid,
        content: &'a str,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn delete_document_by_path<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        path: &'a str,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn list_directory<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        directory: &'a str,
    ) -> impl Future<Output = Result<Vec<WorkspaceEntry>, WorkspaceError>> + Send + 'a;
    fn list_all_paths<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<String>, WorkspaceError>> + Send + 'a;
    fn list_documents<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<MemoryDocument>, WorkspaceError>> + Send + 'a;
    fn delete_chunks<'a>(
        &'a self,
        document_id: Uuid,
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn insert_chunk<'a>(
        &'a self,
        params: InsertChunkParams<'a>,
    ) -> impl Future<Output = Result<Uuid, WorkspaceError>> + Send + 'a;
    fn update_chunk_embedding<'a>(
        &'a self,
        chunk_id: Uuid,
        embedding: &'a [f32],
    ) -> impl Future<Output = Result<(), WorkspaceError>> + Send + 'a;
    fn get_chunks_without_embeddings<'a>(
        &'a self,
        user_id: &'a str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<MemoryChunk>, WorkspaceError>> + Send + 'a;
    fn hybrid_search<'a>(
        &'a self,
        params: HybridSearchParams<'a>,
    ) -> impl Future<Output = Result<Vec<SearchResult>, WorkspaceError>> + Send + 'a;
}
