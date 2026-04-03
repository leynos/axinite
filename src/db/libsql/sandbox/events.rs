//! libSQL sandbox job-event helpers.

use libsql::params;
use uuid::Uuid;

use crate::db::SandboxEventType;
use crate::db::libsql::{LibSqlBackend, get_i64, get_json, get_text, get_ts};
use crate::error::DatabaseError;
use crate::history::JobEventRecord;

enum JobEventsQueryKind {
    BeforeWithLimit,
    BeforeOnly,
    LimitOnly,
    All,
}

fn build_list_job_events_query(
    before_id: Option<i64>,
    limit: Option<i64>,
) -> (&'static str, JobEventsQueryKind) {
    match (before_id, limit) {
        (Some(_), Some(_)) => (
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
            JobEventsQueryKind::BeforeWithLimit,
        ),
        (Some(_), None) => (
            r#"
            SELECT id, job_id, event_type, data, created_at
            FROM job_events WHERE job_id = ?1 AND id < ?2 ORDER BY id ASC
            "#,
            JobEventsQueryKind::BeforeOnly,
        ),
        (None, Some(_)) => (
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
            JobEventsQueryKind::LimitOnly,
        ),
        (None, None) => (
            r#"
            SELECT id, job_id, event_type, data, created_at
            FROM job_events WHERE job_id = ?1 ORDER BY id ASC
            "#,
            JobEventsQueryKind::All,
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
    let (query, kind) = build_list_job_events_query(before_id, limit);
    let mut rows = match (kind, before_id, limit) {
        (JobEventsQueryKind::BeforeWithLimit, Some(before_id), Some(limit)) => conn
            .query(query, params![job_id.as_str(), before_id, limit])
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?,
        (JobEventsQueryKind::BeforeOnly, Some(before_id), None) => conn
            .query(query, params![job_id.as_str(), before_id])
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?,
        (JobEventsQueryKind::LimitOnly, None, Some(limit)) => conn
            .query(query, params![job_id.as_str(), limit])
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?,
        (JobEventsQueryKind::All, None, None) => conn
            .query(query, params![job_id.as_str()])
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?,
        _ => {
            return Err(DatabaseError::Query(
                "invalid job event pagination parameters".to_string(),
            ));
        }
    };

    let mut events = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        events.push(JobEventRecord {
            id: get_i64(&row, 0),
            job_id: get_text(&row, 1).parse().unwrap_or_default(),
            event_type: get_text(&row, 2),
            data: get_json(&row, 3),
            created_at: get_ts(&row, 4),
        });
    }
    Ok(events)
}
