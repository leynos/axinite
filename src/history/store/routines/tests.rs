//! PostgreSQL-backed routine persistence tests.
//!
//! These tests exercise `Store` CRUD, run-history, and due-cron lease
//! behaviour against the `try_test_pg_db` integration database.

use chrono::Utc;
use rstest::{fixture, rstest};
use uuid::Uuid;

use super::Store;
use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineGuardrails, RoutineRun, RunStatus, Trigger,
};
use crate::db::RoutineRunCompletion;
use crate::testing::postgres::try_test_pg_db;

fn sample_routine() -> Routine {
    let now = Utc::now();
    Routine {
        id: Uuid::new_v4(),
        name: format!("routine-{}", Uuid::new_v4()),
        description: "routine persistence test".to_string(),
        user_id: format!("routine-user-{}", Uuid::new_v4()),
        enabled: true,
        trigger: Trigger::Cron {
            schedule: "0 * * * *".to_string(),
            timezone: None,
        },
        action: RoutineAction::Lightweight {
            prompt: "check status".to_string(),
            context_paths: vec![],
            max_tokens: 128,
        },
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(60),
            max_concurrent: 1,
            dedup_window: Some(std::time::Duration::from_secs(300)),
        },
        notify: NotifyConfig {
            channel: None,
            user: "default".to_string(),
            on_attention: true,
            on_failure: true,
            on_success: false,
        },
        last_run_at: None,
        next_fire_at: Some(now),
        run_count: 0,
        consecutive_failures: 0,
        state: serde_json::json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn sample_run(routine_id: Uuid) -> RoutineRun {
    let now = Utc::now();
    RoutineRun {
        id: Uuid::new_v4(),
        routine_id,
        trigger_type: "cron".to_string(),
        trigger_detail: Some("0 * * * *".to_string()),
        started_at: now,
        completed_at: None,
        status: RunStatus::Running,
        result_summary: None,
        tokens_used: None,
        job_id: None,
        created_at: now,
    }
}

async fn cleanup(store: &Store, routine_id: Uuid) {
    let conn = store.conn().await.expect("conn should succeed");
    conn.execute(
        "DELETE FROM routine_runs WHERE routine_id = $1",
        &[&routine_id],
    )
    .await
    .expect("delete routine_runs should succeed");
    conn.execute("DELETE FROM routines WHERE id = $1", &[&routine_id])
        .await
        .expect("delete routines should succeed");
}

#[fixture]
async fn store() -> Option<Store> {
    let backend = try_test_pg_db().await?;
    Some(Store::from_pool(backend.pool()))
}

#[rstest]
#[tokio::test]
async fn routine_crud_and_run_history_round_trip(#[future] store: Option<Store>) {
    let Some(store) = store.await else { return };
    let mut routine = sample_routine();

    store
        .create_routine(&routine)
        .await
        .expect("create_routine should succeed");

    let fetched = store
        .get_routine(routine.id)
        .await
        .expect("get_routine should succeed")
        .expect("routine should exist");
    assert_eq!(fetched.id, routine.id);
    assert_eq!(fetched.name, routine.name);

    let list = store
        .list_routines(&routine.user_id)
        .await
        .expect("list_routines should succeed");
    assert_eq!(list.len(), 1);

    routine.enabled = false;
    routine.description = "updated description".to_string();
    store
        .update_routine(&routine)
        .await
        .expect("update_routine should succeed");

    let updated = store
        .get_routine_by_name(&routine.user_id, &routine.name)
        .await
        .expect("get_routine_by_name should succeed")
        .expect("routine should still exist");
    assert!(!updated.enabled);
    assert_eq!(updated.description, "updated description");

    let run = sample_run(routine.id);
    store
        .create_routine_run(&run)
        .await
        .expect("create_routine_run should succeed");
    store
        .complete_routine_run(RoutineRunCompletion {
            id: run.id,
            status: RunStatus::Ok,
            result_summary: Some("completed"),
            tokens_used: Some(42),
        })
        .await
        .expect("complete_routine_run should succeed");

    let runs = store
        .list_routine_runs(routine.id, 10)
        .await
        .expect("list_routine_runs should succeed");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, run.id);
    assert_eq!(runs[0].result_summary.as_deref(), Some("completed"));
    assert_eq!(runs[0].tokens_used, Some(42));
    assert!(runs[0].completed_at.is_some());

    let deleted = store
        .delete_routine(routine.id)
        .await
        .expect("delete_routine should succeed");
    assert!(deleted);
}

#[rstest]
#[tokio::test]
async fn list_due_cron_routines_claims_and_defers_next_fire_at(#[future] store: Option<Store>) {
    let Some(store) = store.await else { return };
    let mut routine = sample_routine();
    routine.next_fire_at = Some(Utc::now() - chrono::Duration::minutes(1));

    store
        .create_routine(&routine)
        .await
        .expect("create_routine should succeed");

    let due = store
        .list_due_cron_routines()
        .await
        .expect("list_due_cron_routines should succeed");
    assert!(due.iter().any(|due_routine| due_routine.id == routine.id));

    let after_claim = store
        .get_routine(routine.id)
        .await
        .expect("get_routine should succeed")
        .expect("routine should exist");
    let leased_until = after_claim
        .next_fire_at
        .expect("claimed routine should retain a recoverable lease time");
    assert!(leased_until > Utc::now());

    cleanup(&store, routine.id).await;
}
