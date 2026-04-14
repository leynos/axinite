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

/// Identifies the user/agent context for a workspace document query.
///
/// Bundles the `user_id` + `agent_id` pair that every document-scoped
/// helper requires, reducing per-function arity and making call sites
/// self-documenting.
#[derive(Clone, Copy)]
pub(super) struct AgentScope<'a> {
    pub(super) user_id: &'a str,
    pub(super) agent_id: Option<uuid::Uuid>,
}

async fn connect_backend(backend: &LibSqlBackend) -> Result<libsql::Connection, WorkspaceError> {
    backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })
}

async fn fetch_first_row(mut rows: libsql::Rows) -> Result<Option<libsql::Row>, WorkspaceError> {
    rows.next().await.map_err(|e| WorkspaceError::SearchFailed {
        reason: format!("Query failed: {}", e),
    })
}

/// Maps an optional row to a [`MemoryDocument`], returning `not_found` when
/// the row is absent.
fn document_from_row_or_not_found(
    row: Option<libsql::Row>,
    not_found: WorkspaceError,
) -> Result<MemoryDocument, WorkspaceError> {
    match row {
        Some(row) => Ok(row_to_memory_document(&row)),
        None => Err(not_found),
    }
}

async fn drain_rows<T, F>(mut rows: libsql::Rows, map_row: F) -> Result<Vec<T>, WorkspaceError>
where
    F: Fn(libsql::Row) -> T,
{
    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?
    {
        out.push(map_row(row));
    }
    Ok(out)
}

pub(super) async fn get_document_by_path(
    backend: &LibSqlBackend,
    scope: &AgentScope<'_>,
    path: &str,
) -> Result<MemoryDocument, WorkspaceError> {
    let conn = connect_backend(backend).await?;
    let agent_id_str = scope.agent_id.map(|id| id.to_string());
    let rows = conn
        .query(
            r#"
            SELECT id, user_id, agent_id, path, content,
                   created_at, updated_at, metadata
            FROM memory_documents
            WHERE user_id = ?1 AND agent_id IS ?2 AND path = ?3
            "#,
            params![scope.user_id, agent_id_str.as_deref(), path],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?;

    document_from_row_or_not_found(
        fetch_first_row(rows).await?,
        WorkspaceError::DocumentNotFound {
            doc_type: path.to_string(),
            user_id: scope.user_id.to_string(),
        },
    )
}

pub(super) async fn get_document_by_id(
    backend: &LibSqlBackend,
    id: Uuid,
) -> Result<MemoryDocument, WorkspaceError> {
    let conn = connect_backend(backend).await?;
    let rows = conn
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

    document_from_row_or_not_found(
        fetch_first_row(rows).await?,
        WorkspaceError::DocumentNotFound {
            doc_type: "unknown".to_string(),
            user_id: "unknown".to_string(),
        },
    )
}

pub(super) async fn get_or_create_document_by_path(
    backend: &LibSqlBackend,
    scope: &AgentScope<'_>,
    path: &str,
) -> Result<MemoryDocument, WorkspaceError> {
    match NativeWorkspaceStore::get_document_by_path(backend, scope.user_id, scope.agent_id, path)
        .await
    {
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
    let agent_id_str = scope.agent_id.map(|id| id.to_string());
    conn.execute(
        r#"
            INSERT INTO memory_documents (id, user_id, agent_id, path, content, metadata)
            VALUES (?1, ?2, ?3, ?4, '', '{}')
            ON CONFLICT DO NOTHING
            "#,
        params![id.to_string(), scope.user_id, agent_id_str.as_deref(), path],
    )
    .await
    .map_err(|e| WorkspaceError::SearchFailed {
        reason: format!("Insert failed: {}", e),
    })?;

    NativeWorkspaceStore::get_document_by_path(backend, scope.user_id, scope.agent_id, path).await
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
    scope: &AgentScope<'_>,
    path: &str,
) -> Result<(), WorkspaceError> {
    let conn = connect_backend(backend).await?;
    let agent_id_str = scope.agent_id.map(|id| id.to_string());
    let tx = conn
        .transaction()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Delete failed: {}", e),
        })?;
    let rows = tx
        .query(
            r#"
            SELECT id, user_id, agent_id, path, content,
                   created_at, updated_at, metadata
            FROM memory_documents
            WHERE user_id = ?1 AND agent_id IS ?2 AND path = ?3
            "#,
            params![scope.user_id, agent_id_str.as_deref(), path],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?;
    let doc = match fetch_first_row(rows).await? {
        Some(row) => row_to_memory_document(&row),
        None => {
            return Err(WorkspaceError::DocumentNotFound {
                doc_type: path.to_string(),
                user_id: scope.user_id.to_string(),
            });
        }
    };

    tx.execute(
        "DELETE FROM memory_chunks WHERE document_id = ?1",
        params![doc.id.to_string()],
    )
    .await
    .map_err(|e| WorkspaceError::SearchFailed {
        reason: format!("Delete failed: {}", e),
    })?;
    tx.execute(
        "DELETE FROM memory_documents WHERE user_id = ?1 AND agent_id IS ?2 AND path = ?3",
        params![scope.user_id, agent_id_str.as_deref(), path],
    )
    .await
    .map_err(|e| WorkspaceError::SearchFailed {
        reason: format!("Delete failed: {}", e),
    })?;
    tx.commit()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Delete failed: {}", e),
        })?;
    Ok(())
}

fn normalise_dir_prefix(directory: &str) -> String {
    if !directory.is_empty() && !directory.ends_with('/') {
        format!("{}/", directory)
    } else {
        directory.to_string()
    }
}

fn dir_like_pattern(dir: &str) -> String {
    if dir.is_empty() {
        "%".to_string()
    } else {
        format!("{}%", dir)
    }
}

fn resolve_entry(full_path: &str, dir: &str) -> Option<(String, bool, String)> {
    let relative = if dir.is_empty() {
        full_path
    } else {
        full_path.strip_prefix(dir)?
    };
    let child_name = if let Some(slash_pos) = relative.find('/') {
        &relative[..slash_pos]
    } else {
        relative
    };
    if child_name.is_empty() {
        return None;
    }
    let is_dir = relative.contains('/');
    let entry_path = if dir.is_empty() {
        child_name.to_string()
    } else {
        format!("{}{}", dir, child_name)
    };
    Some((child_name.to_string(), is_dir, entry_path))
}

fn merge_entry(
    entries_map: &mut HashMap<String, WorkspaceEntry>,
    child_name: String,
    entry_path: String,
    is_dir: bool,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    content_preview: Option<String>,
) {
    entries_map
        .entry(child_name)
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

pub(super) async fn list_directory(
    backend: &LibSqlBackend,
    scope: &AgentScope<'_>,
    directory: &str,
) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: e.to_string(),
        })?;
    let dir = normalise_dir_prefix(directory);

    let agent_id_str = scope.agent_id.map(|id| id.to_string());
    let pattern = dir_like_pattern(&dir);

    let mut rows = conn
        .query(
            r#"
            SELECT path, updated_at, substr(content, 1, 200) as content_preview
            FROM memory_documents
            WHERE user_id = ?1 AND agent_id IS ?2
              AND (?3 = '%' OR path LIKE ?3)
            ORDER BY path
            "#,
            params![scope.user_id, agent_id_str.as_deref(), pattern],
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
        let Some((child_name, is_dir, entry_path)) = resolve_entry(&full_path, &dir) else {
            continue;
        };
        merge_entry(
            &mut entries_map,
            child_name,
            entry_path,
            is_dir,
            get_opt_ts(&row, 1),
            get_opt_text(&row, 2),
        );
    }

    let mut entries: Vec<WorkspaceEntry> = entries_map.into_values().collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

pub(super) async fn list_all_paths(
    backend: &LibSqlBackend,
    scope: &AgentScope<'_>,
) -> Result<Vec<String>, WorkspaceError> {
    let conn = connect_backend(backend).await?;
    let agent_id_str = scope.agent_id.map(|id| id.to_string());
    let rows = conn
        .query(
            "SELECT path FROM memory_documents WHERE user_id = ?1 AND agent_id IS ?2 ORDER BY path",
            params![scope.user_id, agent_id_str.as_deref()],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("List paths failed: {}", e),
        })?;

    drain_rows(rows, |row| get_text(&row, 0)).await
}

pub(super) async fn list_documents(
    backend: &LibSqlBackend,
    scope: &AgentScope<'_>,
) -> Result<Vec<MemoryDocument>, WorkspaceError> {
    let conn = connect_backend(backend).await?;
    let agent_id_str = scope.agent_id.map(|id| id.to_string());
    let rows = conn
        .query(
            r#"
            SELECT id, user_id, agent_id, path, content,
                   created_at, updated_at, metadata
            FROM memory_documents
            WHERE user_id = ?1 AND agent_id IS ?2
            ORDER BY updated_at DESC
            "#,
            params![scope.user_id, agent_id_str.as_deref()],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Query failed: {}", e),
        })?;

    drain_rows(rows, |row| row_to_memory_document(&row)).await
}
