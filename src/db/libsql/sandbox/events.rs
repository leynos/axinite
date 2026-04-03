//! libSQL sandbox job-event helpers.

use libsql::params;
use uuid::Uuid;

use crate::db::SandboxEventType;
use crate::db::libsql::{LibSqlBackend, get_i64, get_json, get_text, get_ts};
use crate::error::DatabaseError;
use crate::history::JobEventRecord;

fn build_list_job_events_query(
    job_id: &str,
    before_id: Option<i64>,
    limit: Option<i64>,
) -> (&'static str, libsql::params::Params) {
    match (before_id, limit) {
        (Some(before_id), Some(limit)) => (
            r#"
            SELECT id, job_id, event_type, data, created_at
            FROM (
                SELECT id, job_id, event_type, data, created_at
                FROM job_events WHERE job_id = ?1 AND id < ?2
                ORDER BY id DESC
                LIMIT ?3
            )
            ORDER BY id ASC
            "#,
            libsql::params::Params::Positional(vec![job_id.into(), before_id.into(), limit.into()]),
        ),
        (Some(before_id), None) => (
            r#"
            SELECT id, job_id, event_type, data, created_at
            FROM job_events WHERE job_id = ?1 AND id < ?2 ORDER BY id ASC
            "#,
            libsql::params::Params::Positional(vec![job_id.into(), before_id.into()]),
        ),
        (None, Some(limit)) => (
            r#"
            SELECT id, job_id, event_type, data, created_at
            FROM (
                SELECT id, job_id, event_type, data, created_at
                FROM job_events WHERE job_id = ?1
                ORDER BY id DESC
                LIMIT ?2
            )
            ORDER BY id ASC
            "#,
            libsql::params::Params::Positional(vec![job_id.into(), limit.into()]),
        ),
        (None, None) => (
            r#"
            SELECT id, job_id, event_type, data, created_at
            FROM job_events WHERE job_id = ?1 ORDER BY id ASC
            "#,
            libsql::params::Params::Positional(vec![job_id.into()]),
        ),
    }
}

pub(super) async fn save_job_event(
    backend: &LibSqlBackend,
    job_id: Uuid,
    event_type: SandboxEventType,
    data: &serde_json::Value,
) -> Result<(), DatabaseError> {
    let conn = backend.connect().await?;
    conn.execute(
        "INSERT INTO job_events (job_id, event_type, data) VALUES (?1, ?2, ?3)",
        params![job_id.to_string(), event_type.as_str(), data.to_string()],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn list_job_events(
    backend: &LibSqlBackend,
    job_id: Uuid,
    before_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<JobEventRecord>, DatabaseError> {
    let conn = backend.connect().await?;
    let job_id = job_id.to_string();
    let (query, query_params) = build_list_job_events_query(&job_id, before_id, limit);
    let mut rows = conn
        .query(query, query_params)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut events = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        let job_id = get_text(&row, 1).parse().map_err(|e| {
            DatabaseError::Serialization(format!("invalid job_events.job_id UUID: {e}"))
        })?;
        events.push(JobEventRecord {
            id: get_i64(&row, 0),
            job_id,
            event_type: get_text(&row, 2),
            data: get_json(&row, 3),
            created_at: get_ts(&row, 4),
        });
    }
    Ok(events)
}
