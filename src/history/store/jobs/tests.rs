//! Unit tests for job history persistence.

use super::*;
#[cfg(feature = "postgres")]
use crate::context::StateTransition;
#[cfg(feature = "postgres")]
use crate::db::TerminalJobPersistence;
#[cfg(feature = "postgres")]
use crate::db::postgres::PgBackend;
#[cfg(feature = "postgres")]
use crate::testing::postgres::try_test_pg_db;
#[cfg(feature = "postgres")]
use rstest::rstest;
#[cfg(feature = "postgres")]
use serde_json::json;

#[cfg(feature = "postgres")]
enum RollbackScenario {
    UnknownJob,
    NonDirectJob,
}

#[cfg(feature = "postgres")]
async fn prepare_job_for_rollback(
    backend: &PgBackend,
    store: &Store,
    scenario: RollbackScenario,
) -> Result<(Uuid, Option<JobContext>), Box<dyn std::error::Error>> {
    match scenario {
        RollbackScenario::UnknownJob => Ok((Uuid::new_v4(), None)),
        RollbackScenario::NonDirectJob => {
            let ctx = JobContext::with_user("test-user", "sandbox-like job", "rollback check");
            let job_id = ctx.job_id;
            store.save_job(&ctx).await?;

            let conn = backend.pool().get().await?;
            conn.execute(
                "UPDATE agent_jobs SET source = 'sandbox' WHERE id = $1",
                &[&job_id],
            )
            .await?;

            Ok((job_id, Some(ctx)))
        }
    }
}

/// Regression test: save_job must persist user-owned and context fields.
/// Requires a running PostgreSQL instance (integration tier).
#[cfg(feature = "postgres")]
#[rstest]
#[tokio::test]
async fn test_save_job_persists_user_id() {
    use crate::context::JobContext;

    let Some(backend) = try_test_pg_db()
        .await
        .expect("unexpected Postgres test setup error")
    else {
        return;
    };
    let store = Store::from_pool(backend.pool());

    let ctx = JobContext::with_user("test-user-42", "PG user_id test", "regression test");
    let mut ctx = ctx.with_timezone("Europe/London");
    ctx.metadata = serde_json::json!({ "mode": "regression" });
    ctx.transitions.push(StateTransition {
        from: JobState::Pending,
        to: JobState::InProgress,
        timestamp: Utc::now(),
        reason: Some("test".to_string()),
    });
    store.save_job(&ctx).await.expect("save_job should succeed");

    let loaded = store
        .get_job(ctx.job_id)
        .await
        .expect("get_job should succeed")
        .expect("job should exist");
    assert_eq!(loaded.user_id, "test-user-42");
    assert_eq!(loaded.user_timezone, "Europe/London");
    assert_eq!(loaded.metadata, ctx.metadata);
    assert_eq!(loaded.transitions.len(), 1);

    let conn = backend.pool().get().await.expect("pool get should succeed");
    conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&ctx.job_id])
        .await
        .expect("delete agent_jobs should succeed");
}

#[test]
fn agent_job_summary_accumulates_status_buckets() {
    let mut summary = AgentJobSummary::default();
    summary.add_count("pending", 1);
    summary.add_count("submitted", 6);
    summary.add_count("accepted", 7);
    summary.add_count("in_progress", 2);
    summary.add_count("completed", 3);
    summary.add_count("failed", 4);
    summary.add_count("stuck", 5);
    summary.add_count("cancelled", 8);

    assert_eq!(summary.total, 36);
    assert_eq!(summary.pending, 1);
    assert_eq!(summary.in_progress, 2);
    assert_eq!(summary.completed, 16);
    assert_eq!(summary.failed, 12);
    assert_eq!(summary.stuck, 5);
}

#[cfg(feature = "postgres")]
#[rstest]
#[case(RollbackScenario::UnknownJob)]
#[case(RollbackScenario::NonDirectJob)]
#[tokio::test]
async fn persist_terminal_result_and_status_rolls_back_on_invalid_job(
    #[case] scenario: RollbackScenario,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(backend) = try_test_pg_db().await? else {
        return Ok(());
    };
    let store = Store::from_pool(backend.pool());
    let (job_id, saved_ctx) = prepare_job_for_rollback(&backend, &store, scenario).await?;

    let result = store
        .persist_terminal_result_and_status(TerminalJobPersistence {
            job_id,
            status: JobState::Failed,
            failure_reason: Some("terminal rollback regression"),
            event_type: crate::db::SandboxEventType::from("result"),
            event_data: &json!({"status": "failed"}),
        })
        .await;
    assert!(result.is_err(), "invalid terminal job write should fail");

    let conn = backend.pool().get().await?;
    let count: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM job_events WHERE job_id = $1",
            &[&job_id],
        )
        .await?
        .get(0);
    assert_eq!(count, 0, "rollback should remove inserted job_events rows");
    if let Some(ctx) = saved_ctx {
        conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&ctx.job_id])
            .await?;
    }
    Ok(())
}
