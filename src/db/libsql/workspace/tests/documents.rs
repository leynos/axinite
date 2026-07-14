//! Tests for document and chunk CRUD, listing, and error paths.

use libsql::params;

use super::helpers::{assert_document_not_found, create_test_document, setup_backend};
use crate::db::{InsertChunkParams, NativeWorkspaceStore};

#[tokio::test]
async fn get_chunks_without_embeddings_skips_invalid_chunk_id_uuid() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/bad-chunk-uuid.md")
        .await
        .expect("failed to create document");

    let conn = backend.connect().await.expect("failed to connect");
    conn.execute(
        "INSERT INTO memory_chunks (id, document_id, chunk_index, content, created_at) \
         VALUES ('not-a-uuid', ?1, 0, 'bad chunk', datetime('now'))",
        params![document.id.to_string()],
    )
    .await
    .expect("failed to insert bad-chunk-id row");

    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 1,
            content: "valid chunk",
            embedding: None,
        })
        .await
        .expect("failed to insert valid chunk");

    let chunks = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("get_chunks_without_embeddings must not fail on invalid UUIDs");

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content, "valid chunk");
}

#[tokio::test]
async fn get_chunks_without_embeddings_errors_on_negative_chunk_index() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/neg-idx.md")
        .await
        .expect("failed to create document");

    let conn = backend.connect().await.expect("failed to connect");
    conn.execute(
        "INSERT INTO memory_chunks (id, document_id, chunk_index, content, created_at) \
         VALUES (?1, ?2, -1, 'negative index', datetime('now'))",
        params![uuid::Uuid::new_v4().to_string(), document.id.to_string()],
    )
    .await
    .expect("failed to insert negative-index row");

    let result = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await;

    assert!(
        result.is_err(),
        "get_chunks_without_embeddings must return Err for negative chunk_index"
    );
}

#[tokio::test]
async fn get_document_by_path_returns_not_found_for_missing_document() {
    let backend = setup_backend().await.expect("failed to set up backend");
    let result = backend
        .get_document_by_path("default", None, "does/not/exist.md")
        .await;
    assert_document_not_found(result);
}

#[tokio::test]
async fn get_document_by_id_returns_not_found_for_unknown_id() {
    let backend = setup_backend().await.expect("failed to set up backend");
    let result = backend.get_document_by_id(uuid::Uuid::new_v4()).await;
    assert_document_not_found(result);
}

#[tokio::test]
async fn insert_chunk_and_delete_chunks_round_trip() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let document = create_test_document(&backend, "notes/chunks.md")
        .await
        .expect("failed to create test document");

    let chunk_id = backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "round-trip chunk",
            embedding: None,
        })
        .await
        .expect("failed to insert chunk");

    let before = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks before delete");
    assert!(
        before.iter().any(|c| c.id == chunk_id),
        "inserted chunk must appear in get_chunks_without_embeddings"
    );

    backend
        .delete_chunks(document.id)
        .await
        .expect("failed to delete chunks");

    let after = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks after delete");
    assert!(
        after.iter().all(|c| c.id != chunk_id),
        "deleted chunk must not appear after delete_chunks"
    );
}

#[tokio::test]
async fn update_chunk_embedding_is_reflected_in_chunks_list() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let document = create_test_document(&backend, "notes/embed-update.md")
        .await
        .expect("failed to create test document");

    let chunk_id = backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "embedding update test",
            embedding: None,
        })
        .await
        .expect("failed to insert chunk");

    let before = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks before embedding update");
    assert!(
        before.iter().any(|c| c.id == chunk_id),
        "chunk without embedding must appear before update"
    );

    backend
        .update_chunk_embedding(chunk_id, &[0.1, 0.2, 0.3])
        .await
        .expect("failed to update chunk embedding");

    let after = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks after embedding update");
    assert!(
        after.iter().all(|c| c.id != chunk_id),
        "chunk with embedding must not appear in get_chunks_without_embeddings"
    );
}

#[tokio::test]
async fn get_or_create_document_by_path_is_idempotent() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let first = backend
        .get_or_create_document_by_path("default", None, "notes/idempotent.md")
        .await
        .expect("failed to create document on first call");
    let second = backend
        .get_or_create_document_by_path("default", None, "notes/idempotent.md")
        .await
        .expect("failed to get document on second call");

    assert_eq!(first.id, second.id, "get_or_create must return the same id");
}

#[tokio::test]
async fn update_document_changes_content() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/update.md")
        .await
        .expect("failed to create document");
    backend
        .update_document(document.id, "updated content")
        .await
        .expect("failed to update document content");

    let fetched = backend
        .get_document_by_id(document.id)
        .await
        .expect("failed to fetch updated document");
    assert_eq!(
        fetched.content, "updated content",
        "document content must reflect update"
    );
}

#[tokio::test]
async fn delete_document_by_path_removes_document_and_chunks() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/delete-me.md")
        .await
        .expect("failed to create document");
    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "to be deleted",
            embedding: None,
        })
        .await
        .expect("failed to insert chunk");

    backend
        .delete_document_by_path("default", None, "notes/delete-me.md")
        .await
        .expect("failed to delete document");

    let result = backend
        .get_document_by_path("default", None, "notes/delete-me.md")
        .await;
    assert_document_not_found(result);

    let chunks = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks after document deletion");
    assert!(
        chunks.iter().all(|c| c.document_id != document.id),
        "chunks belonging to deleted document must be removed"
    );
}

#[tokio::test]
async fn list_all_paths_returns_inserted_document_path() {
    let backend = setup_backend().await.expect("failed to set up backend");

    create_test_document(&backend, "notes/listed.md")
        .await
        .expect("failed to create test document");

    let paths = backend
        .list_all_paths("default", None)
        .await
        .expect("failed to list all paths");

    assert!(
        paths.contains(&"notes/listed.md".to_string()),
        "list_all_paths must include inserted document path"
    );
}

#[tokio::test]
async fn list_documents_returns_inserted_document() {
    let backend = setup_backend().await.expect("failed to set up backend");

    let document = create_test_document(&backend, "notes/listed-doc.md")
        .await
        .expect("failed to create test document");

    let docs = backend
        .list_documents("default", None)
        .await
        .expect("failed to list documents");

    assert!(
        docs.iter().any(|d| d.id == document.id),
        "list_documents must include inserted document"
    );
}

#[tokio::test]
async fn list_directory_returns_immediate_children_only() {
    let backend = setup_backend().await.expect("failed to set up backend");

    backend
        .get_or_create_document_by_path("default", None, "notes/dir/child.md")
        .await
        .expect("failed to create child document");
    backend
        .get_or_create_document_by_path("default", None, "notes/dir/sub/deep.md")
        .await
        .expect("failed to create deeply nested document");

    let entries = backend
        .list_directory("default", None, "notes/dir")
        .await
        .expect("failed to list directory");

    assert!(
        entries
            .iter()
            .any(|e| !e.is_directory && e.path.ends_with("child.md")),
        "list_directory must include the direct file child"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.is_directory && e.path.ends_with("sub")),
        "list_directory must include the sub-directory child"
    );
    assert!(
        entries.iter().all(|e| !e.path.ends_with("deep.md")),
        "list_directory must not include deeply nested files"
    );
}
