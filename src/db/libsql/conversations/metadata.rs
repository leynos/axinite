//! LibSQL conversation-metadata helpers.

use super::*;

pub(super) async fn update_conversation_metadata_field(
    backend: &LibSqlBackend,
    id: Uuid,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), DatabaseError> {
    let conn = backend.connect().await?;
    let patch = serde_json::json!({ key: value });
    conn.execute(
        "UPDATE conversations SET metadata = json_patch(metadata, ?2) WHERE id = ?1",
        params![id.to_string(), patch.to_string()],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn get_conversation_metadata(
    backend: &LibSqlBackend,
    id: Uuid,
) -> Result<Option<serde_json::Value>, DatabaseError> {
    let conn = backend.connect().await?;
    let mut rows = conn
        .query(
            "SELECT metadata FROM conversations WHERE id = ?1",
            params![id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    match rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        Some(row) => Ok(Some(get_json(&row, 0))),
        None => Ok(None),
    }
}

pub(super) async fn conversation_belongs_to_user(
    backend: &LibSqlBackend,
    conversation_id: Uuid,
    user_id: &str,
) -> Result<bool, DatabaseError> {
    let conn = backend.connect().await?;
    let mut rows = conn
        .query(
            "SELECT 1 FROM conversations WHERE id = ?1 AND user_id = ?2",
            params![conversation_id.to_string(), user_id],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    let found = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(found.is_some())
}
