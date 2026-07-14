//! Unit tests for libSQL job persistence and event recording.

use super::*;
use crate::db::NativeDatabase;
use crate::db::SandboxEventType;
use anyhow::Context as _;
use chrono::Utc;
use serde_json::json;

async fn count_job_events(backend: &LibSqlBackend, job_id: Uuid) -> anyhow::Result<i64> {
    let conn = backend
        .connect()
        .await
        .context("connection should succeed")?;
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM job_events WHERE job_id = ?1",
            params![job_id.to_string()],
        )
        .await
        .context("count query should succeed")?;
    let row = rows
        .next()
        .await
        .context("count row should load")?
        .context("count row should exist")?;
    row.get::<i64>(0).context("count column should decode")
}

async fn seed_non_direct_job(backend: &LibSqlBackend, job_id: Uuid) -> anyhow::Result<()> {
    let conn = backend
        .connect()
        .await
        .context("connection should succeed")?;
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
    .context("sandbox job should seed")?;
    Ok(())
}

#[tokio::test]
async fn persist_terminal_result_and_status_rejects_unknown_job_ids() {
    let dir = tempfile::tempdir().expect("tempdir should succeed");
    let db_path = dir.path().join("jobs.sqlite");
    let backend = LibSqlBackend::new_local(&db_path)
        .await
        .expect("new_local should succeed");
    backend
        .run_migrations()
        .await
        .expect("migrations should succeed");

    let job_id = Uuid::new_v4();
    let result = backend
        .persist_terminal_result_and_status(TerminalJobPersistence {
            job_id,
            status: JobState::Completed,
            failure_reason: None,
            event_type: SandboxEventType::from("result"),
            event_data: &json!({"status": "completed"}),
        })
        .await;

    assert!(result.is_err(), "unknown job ID should fail");
    let event_count = count_job_events(&backend, job_id)
        .await
        .expect("counting job events should succeed");
    assert_eq!(
        event_count, 0,
        "unknown job ID should not leave a terminal event behind"
    );

    let sandbox_job_id = Uuid::new_v4();
    seed_non_direct_job(&backend, sandbox_job_id)
        .await
        .expect("sandbox job should seed");

    let sandbox_result = backend
        .persist_terminal_result_and_status(TerminalJobPersistence {
            job_id: sandbox_job_id,
            status: JobState::Completed,
            failure_reason: None,
            event_type: SandboxEventType::from("result"),
            event_data: &json!({"status": "completed"}),
        })
        .await;

    assert!(
        sandbox_result.is_err(),
        "non-direct job ID should fail terminal persistence"
    );
    let sandbox_event_count = count_job_events(&backend, sandbox_job_id)
        .await
        .expect("counting job events should succeed");
    assert_eq!(
        sandbox_event_count, 0,
        "non-direct job ID should not leave a terminal event behind"
    );
}
