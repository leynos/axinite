//! Document-oriented workspace-store helpers for the libSQL backend.

use std::collections::HashMap;

use chrono::Utc;
use libsql::params;
use uuid::Uuid;

use super::super::{
    LibSqlBackend, fmt_ts, get_opt_text, get_opt_ts, get_text, row_to_memory_document,
};
use crate::db::NativeWorkspaceStore;
use crate::error::WorkspaceError;
use crate::workspace::{MemoryDocument, WorkspaceEntry};

pub(super) async fn get_document_by_path(
    backend: &LibSqlBackend,
    user_id: &str,
    agent_id: Option<Uuid>,
    path: &str,
) -> Result<MemoryDocument, WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let agent_id_str = agent_id.map(|id| id.to_string());
    let mut rows = conn
        .query(
            r#"
            SELECT id, user_id, agent_id, path, content,
                   created_at, updated_at, metadata
            FROM memory_documents
            WHERE user_id = ?1 AND agent_id IS ?2 AND path = ?3
            "#,
            params![user_id, agent_id_str.as_deref(), path],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?;

    match rows
        .next()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })? {
        Some(row) => Ok(row_to_memory_document(&row)),
        None => Err(WorkspaceError::DocumentNotFound {
            doc_type: path.to_string(),
            user_id: user_id.to_string(),
        }),
    }
}

pub(super) async fn get_document_by_id(
    backend: &LibSqlBackend,
    id: Uuid,
) -> Result<MemoryDocument, WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let mut rows = conn
        .query(
            r#"
            SELECT id, user_id, agent_id, path, content,
                   created_at, updated_at, metadata
            FROM memory_documents WHERE id = ?1
            "#,
            params![id.to_string()],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?;

    match rows
        .next()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })? {
        Some(row) => Ok(row_to_memory_document(&row)),
        None => Err(WorkspaceError::DocumentNotFound {
            doc_type: "unknown".to_string(),
            user_id: "unknown".to_string(),
        }),
    }
}

pub(super) async fn get_or_create_document_by_path(
    backend: &LibSqlBackend,
    user_id: &str,
    agent_id: Option<Uuid>,
    path: &str,
) -> Result<MemoryDocument, WorkspaceError> {
    match NativeWorkspaceStore::get_document_by_path(backend, user_id, agent_id, path).await {
        Ok(doc) => return Ok(doc),
        Err(WorkspaceError::DocumentNotFound { .. }) => {}
        Err(e) => return Err(e),
    }

    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let id = Uuid::new_v4();
    let agent_id_str = agent_id.map(|id| id.to_string());
    conn.execute(
        r#"
            INSERT INTO memory_documents (id, user_id, agent_id, path, content, metadata)
            VALUES (?1, ?2, ?3, ?4, '', '{}')
            ON CONFLICT DO NOTHING
            "#,
        params![id.to_string(), user_id, agent_id_str.as_deref(), path],
    )
    .await
    .map_err(|e| WorkspaceError::SearchFailed {
        reason: format!("Insert failed: {}", e),
    })?;

    NativeWorkspaceStore::get_document_by_path(backend, user_id, agent_id, path).await
}

pub(super) async fn update_document(
    backend: &LibSqlBackend,
    id: Uuid,
    content: &str,
) -> Result<(), WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let now = fmt_ts(&Utc::now());
    conn.execute(
        "UPDATE memory_documents SET content = ?2, updated_at = ?3 WHERE id = ?1",
        params![id.to_string(), content, now],
    )
    .await
    .map_err(|e| WorkspaceError::SearchFailed {
        reason: format!("Update failed: {}", e),
    })?;
    Ok(())
}

pub(super) async fn delete_document_by_path(
    backend: &LibSqlBackend,
    user_id: &str,
    agent_id: Option<Uuid>,
    path: &str,
) -> Result<(), WorkspaceError> {
    let doc = NativeWorkspaceStore::get_document_by_path(backend, user_id, agent_id, path).await?;
    NativeWorkspaceStore::delete_chunks(backend, doc.id).await?;

    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let agent_id_str = agent_id.map(|id| id.to_string());
    conn.execute(
        "DELETE FROM memory_documents WHERE user_id = ?1 AND agent_id IS ?2 AND path = ?3",
        params![user_id, agent_id_str.as_deref(), path],
    )
    .await
    .map_err(|e| WorkspaceError::SearchFailed {
        reason: format!("Delete failed: {}", e),
    })?;
    Ok(())
}

pub(super) async fn list_directory(
    backend: &LibSqlBackend,
    user_id: &str,
    agent_id: Option<Uuid>,
    directory: &str,
) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let dir = if !directory.is_empty() && !directory.ends_with('/') {
        format!("{}/", directory)
    } else {
        directory.to_string()
    };

    let agent_id_str = agent_id.map(|id| id.to_string());
    let pattern = if dir.is_empty() {
        "%".to_string()
    } else {
        format!("{}%", dir)
    };

    let mut rows = conn
        .query(
            r#"
            SELECT path, updated_at, substr(content, 1, 200) as content_preview
            FROM memory_documents
            WHERE user_id = ?1 AND agent_id IS ?2
              AND (?3 = '%' OR path LIKE ?3)
            ORDER BY path
            "#,
            params![user_id, agent_id_str.as_deref(), pattern],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("List directory failed: {}", e),
        })?;

    let mut entries_map: HashMap<String, WorkspaceEntry> = HashMap::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?
    {
        let full_path = get_text(&row, 0);
        let updated_at = get_opt_ts(&row, 1);
        let content_preview = get_opt_text(&row, 2);
        let relative = if dir.is_empty() {
            &full_path
        } else if let Some(stripped) = full_path.strip_prefix(&dir) {
            stripped
        } else {
            continue;
        };

        let child_name = if let Some(slash_pos) = relative.find('/') {
            &relative[..slash_pos]
        } else {
            relative
        };
        if child_name.is_empty() {
            continue;
        }

        let is_dir = relative.contains('/');
        let entry_path = if dir.is_empty() {
            child_name.to_string()
        } else {
            format!("{}{}", dir, child_name)
        };

        entries_map
            .entry(child_name.to_string())
            .and_modify(|entry| {
                if is_dir {
                    entry.is_directory = true;
                    entry.content_preview = None;
                }
                if let (Some(existing), Some(new)) = (&entry.updated_at, &updated_at)
                    && new > existing
                {
                    entry.updated_at = Some(*new);
                }
            })
            .or_insert(WorkspaceEntry {
                path: entry_path,
                is_directory: is_dir,
                updated_at,
                content_preview: if is_dir { None } else { content_preview },
            });
    }

    let mut entries: Vec<WorkspaceEntry> = entries_map.into_values().collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

pub(super) async fn list_all_paths(
    backend: &LibSqlBackend,
    user_id: &str,
    agent_id: Option<Uuid>,
) -> Result<Vec<String>, WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let agent_id_str = agent_id.map(|id| id.to_string());
    let mut rows = conn
        .query(
            "SELECT path FROM memory_documents WHERE user_id = ?1 AND agent_id IS ?2 ORDER BY path",
            params![user_id, agent_id_str.as_deref()],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("List paths failed: {}", e),
        })?;

    let mut paths = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?
    {
        paths.push(get_text(&row, 0));
    }
    Ok(paths)
}

pub(super) async fn list_documents(
    backend: &LibSqlBackend,
    user_id: &str,
    agent_id: Option<Uuid>,
) -> Result<Vec<MemoryDocument>, WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let agent_id_str = agent_id.map(|id| id.to_string());
    let mut rows = conn
        .query(
            r#"
            SELECT id, user_id, agent_id, path, content,
                   created_at, updated_at, metadata
            FROM memory_documents
            WHERE user_id = ?1 AND agent_id IS ?2
            ORDER BY updated_at DESC
            "#,
            params![user_id, agent_id_str.as_deref()],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?;

    let mut docs = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?
    {
        docs.push(row_to_memory_document(&row));
    }
    Ok(docs)
}
