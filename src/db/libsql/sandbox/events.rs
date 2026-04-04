//! libSQL sandbox job-event helpers.

use libsql::params;
use uuid::Uuid;

use crate::db::SandboxEventType;
use crate::db::libsql::{LibSqlBackend, get_i64, get_text, get_ts};
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

/// Persist one sandbox job event row into `job_events`.
///
/// `backend` supplies the libSQL connection, `job_id` identifies the owning
/// sandbox job, `event_type` names the event discriminator, and `data` holds
/// the structured JSON payload. Returns a [`DatabaseError`] when connection
/// acquisition or the insert fails.
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

/// Load sandbox job events using an optional exclusive cursor and limit.
///
/// `before_id` filters to rows with `id < before_id`, while `limit` caps the
/// returned event count and must be positive when provided. Results are returned
/// oldest-to-newest by `id` even when the query pages newest-first internally.
///
/// Returns [`DatabaseError::Query`] for connection or SQL failures, and
/// [`DatabaseError::Serialization`] if a stored `job_id` cannot be parsed as a
/// [`Uuid`] or the JSON payload is malformed.
pub(super) async fn list_job_events(
    backend: &LibSqlBackend,
    job_id: Uuid,
    before_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<JobEventRecord>, DatabaseError> {
    if let Some(limit) = limit
        && limit <= 0
    {
        return Err(DatabaseError::Query(
            "list_job_events limit must be greater than 0".to_string(),
        ));
    }

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
        let data_raw = get_text(&row, 3);
        let data = serde_json::from_str(&data_raw).map_err(|e| {
            DatabaseError::Serialization(format!("invalid job_events.data JSON: {e}"))
        })?;
        events.push(JobEventRecord {
            id: get_i64(&row, 0),
            job_id,
            event_type: SandboxEventType::from(get_text(&row, 2)),
            data,
            created_at: get_ts(&row, 4),
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::TempDir;
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::*;
    use crate::db::NativeDatabase;

    async fn backend() -> (TempDir, LibSqlBackend) {
        let dir = tempdir().expect("tempdir should be created");
        let db_path = dir.path().join("sandbox-events.db");
        let backend = LibSqlBackend::new_local(&db_path)
            .await
            .expect("backend should be created");
        backend
            .run_migrations()
            .await
            .expect("migrations should succeed");
        (dir, backend)
    }

    async fn seed_sandbox_job(backend: &LibSqlBackend, job_id: Uuid) {
        let conn = backend.connect().await.expect("connection should succeed");
        conn.execute(
            r#"
            INSERT INTO agent_jobs (
                id, title, description, status, source, user_id, project_dir, created_at
            ) VALUES (?1, ?2, ?3, ?4, 'sandbox', ?5, ?6, ?7)
            "#,
            params![
                job_id.to_string(),
                "Sandbox test job",
                "{}",
                "creating",
                "test-user",
                "/tmp/test-project",
                Utc::now().to_rfc3339(),
            ],
        )
        .await
        .expect("sandbox job should seed");
    }

    fn assert_positional_params(
        params: libsql::params::Params,
        expected_len: usize,
    ) -> Vec<libsql::Value> {
        match params {
            libsql::params::Params::Positional(values) => {
                assert_eq!(values.len(), expected_len);
                values
            }
            other => panic!("expected positional params, got {other:?}"),
        }
    }

    #[test]
    fn build_list_job_events_query_handles_before_and_limit() {
        let (sql, params) = build_list_job_events_query("job-1", Some(42), Some(10));
        assert!(sql.contains("WHERE job_id = ?1 AND id < ?2"));
        assert!(sql.contains("LIMIT ?3"));
        let values = assert_positional_params(params, 3);
        assert_eq!(values[0], libsql::Value::Text("job-1".to_string()));
        assert_eq!(values[1], libsql::Value::Integer(42));
        assert_eq!(values[2], libsql::Value::Integer(10));
    }

    #[test]
    fn build_list_job_events_query_handles_before_only() {
        let (sql, params) = build_list_job_events_query("job-1", Some(42), None);
        assert!(sql.contains("WHERE job_id = ?1 AND id < ?2 ORDER BY id ASC"));
        let values = assert_positional_params(params, 2);
        assert_eq!(values[0], libsql::Value::Text("job-1".to_string()));
        assert_eq!(values[1], libsql::Value::Integer(42));
    }

    #[test]
    fn build_list_job_events_query_handles_limit_only() {
        let (sql, params) = build_list_job_events_query("job-1", None, Some(10));
        assert!(sql.contains("FROM job_events WHERE job_id = ?1"));
        assert!(sql.contains("LIMIT ?2"));
        let values = assert_positional_params(params, 2);
        assert_eq!(values[0], libsql::Value::Text("job-1".to_string()));
        assert_eq!(values[1], libsql::Value::Integer(10));
    }

    #[test]
    fn build_list_job_events_query_handles_unbounded_history() {
        let (sql, params) = build_list_job_events_query("job-1", None, None);
        assert!(sql.contains("WHERE job_id = ?1 ORDER BY id ASC"));
        let values = assert_positional_params(params, 1);
        assert_eq!(values[0], libsql::Value::Text("job-1".to_string()));
    }

    #[tokio::test]
    async fn list_job_events_returns_ordered_limited_results() {
        let (_dir, backend) = backend().await;
        let job_id = Uuid::new_v4();
        seed_sandbox_job(&backend, job_id).await;

        save_job_event(
            &backend,
            job_id,
            SandboxEventType::from("stdout"),
            &serde_json::json!({"message": "one"}),
        )
        .await
        .expect("first event should save");
        save_job_event(
            &backend,
            job_id,
            SandboxEventType::from("stdout"),
            &serde_json::json!({"message": "two"}),
        )
        .await
        .expect("second event should save");
        save_job_event(
            &backend,
            job_id,
            SandboxEventType::from("stdout"),
            &serde_json::json!({"message": "three"}),
        )
        .await
        .expect("third event should save");

        let all_events = list_job_events(&backend, job_id, None, None)
            .await
            .expect("full event list should load");
        assert_eq!(all_events.len(), 3);
        assert!(
            all_events
                .windows(2)
                .all(|window| window[0].id < window[1].id)
        );

        let limited = list_job_events(&backend, job_id, None, Some(2))
            .await
            .expect("limited event list should load");
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].data["message"], "two");
        assert_eq!(limited[1].data["message"], "three");

        let before = list_job_events(&backend, job_id, Some(all_events[2].id), None)
            .await
            .expect("before cursor event list should load");
        assert_eq!(before.len(), 2);
        assert_eq!(before[0].data["message"], "one");
        assert_eq!(before[1].data["message"], "two");
    }

    #[tokio::test]
    async fn list_job_events_rejects_non_positive_limit() {
        let (_dir, backend) = backend().await;
        let result = list_job_events(&backend, Uuid::new_v4(), None, Some(0)).await;

        assert!(matches!(
            result,
            Err(DatabaseError::Query(message))
                if message == "list_job_events limit must be greater than 0"
        ));
    }

    #[tokio::test]
    async fn list_job_events_returns_serialization_error_for_invalid_json_payload() {
        let (_dir, backend) = backend().await;
        let conn = backend.connect().await.expect("connection should succeed");
        let job_id = Uuid::new_v4();
        seed_sandbox_job(&backend, job_id).await;

        conn.execute(
            "INSERT INTO job_events (job_id, event_type, data) VALUES (?1, ?2, ?3)",
            params![job_id.to_string(), "stdout", "{broken-json"],
        )
        .await
        .expect("corrupt row should insert");

        let result = list_job_events(&backend, job_id, None, None).await;
        assert!(matches!(
            result,
            Err(DatabaseError::Serialization(message))
                if message.contains("invalid job_events.data JSON")
        ));
    }
}
