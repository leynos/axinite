//! Routine CRUD and runtime-update persistence tests.

use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::db::{Database, RoutineRunCompletion, RoutineRuntimeUpdate};
use crate::testing::TestHarnessBuilder;

async fn create_routine_fixture(db: &Arc<dyn Database>) -> Result<uuid::Uuid> {
    use crate::agent::routine::{NotifyConfig, Routine, RoutineAction, RoutineGuardrails, Trigger};

    let routine_id = uuid::Uuid::new_v4();
    let routine = Routine {
        id: routine_id,
        name: "test-routine".to_string(),
        description: "A test routine".to_string(),
        user_id: "user1".to_string(),
        enabled: true,
        trigger: Trigger::Cron {
            schedule: "0 * * * *".to_string(),
            timezone: None,
        },
        action: RoutineAction::Lightweight {
            prompt: "Check status".to_string(),
            context_paths: vec![],
            max_tokens: 500,
        },
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(60),
            max_concurrent: 1,
            dedup_window: None,
        },
        notify: NotifyConfig {
            channel: None,
            user: "user1".to_string(),
            on_attention: true,
            on_failure: true,
            on_success: false,
        },
        last_run_at: None,
        next_fire_at: None,
        run_count: 0,
        consecutive_failures: 0,
        state: serde_json::json!({}),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    db.create_routine(&routine)
        .await
        .context("create routine")?;

    // Get by ID
    let fetched = db
        .get_routine(routine_id)
        .await
        .context("get routine")?
        .context("should exist")?;
    assert_eq!(fetched.name, "test-routine");
    assert!(fetched.enabled);

    // Get by name
    let by_name = db
        .get_routine_by_name("user1", "test-routine")
        .await
        .context("get by name")?
        .context("should exist")?;
    assert_eq!(by_name.id, routine_id);

    // List routines for user
    let list = db.list_routines("user1").await.context("list routines")?;
    assert_eq!(list.len(), 1);

    // List all routines
    let all = db.list_all_routines().await.context("list all")?;
    assert!(!all.is_empty());

    // Update routine (disable + change description)
    let mut updated = fetched;
    updated.enabled = false;
    updated.description = "Updated description".to_string();
    db.update_routine(&updated)
        .await
        .context("update routine")?;

    let re_fetched = db
        .get_routine(routine_id)
        .await
        .context("get")?
        .context("exists")?;
    assert!(!re_fetched.enabled);
    assert_eq!(re_fetched.description, "Updated description");

    Ok(routine_id)
}

async fn start_routine_run(db: &Arc<dyn Database>, routine_id: uuid::Uuid) -> Result<uuid::Uuid> {
    use crate::agent::routine::{RoutineRun, RunStatus};

    let run_id = uuid::Uuid::new_v4();
    let run = RoutineRun {
        id: run_id,
        routine_id,
        trigger_type: "cron".to_string(),
        trigger_detail: Some("0 * * * *".to_string()),
        started_at: chrono::Utc::now(),
        completed_at: None,
        status: RunStatus::Running,
        result_summary: None,
        tokens_used: None,
        job_id: None,
        created_at: chrono::Utc::now(),
    };
    db.create_routine_run(&run).await.context("create run")?;

    // List runs
    let runs = db
        .list_routine_runs(routine_id, 10)
        .await
        .context("list runs")?;
    assert_eq!(runs.len(), 1);
    assert!(matches!(runs[0].status, RunStatus::Running));

    Ok(run_id)
}

async fn complete_routine_run_ok(db: &Arc<dyn Database>, run_id: uuid::Uuid) -> Result<()> {
    use crate::agent::routine::RunStatus;

    db.complete_routine_run(RoutineRunCompletion {
        id: run_id,
        status: RunStatus::Ok,
        result_summary: Some("All good"),
        tokens_used: Some(150),
    })
    .await
    .context("complete run")?;
    Ok(())
}

async fn assert_history_len(
    db: &Arc<dyn Database>,
    routine_id: uuid::Uuid,
    expected: usize,
) -> Result<()> {
    use crate::agent::routine::RunStatus;

    let runs = db
        .list_routine_runs(routine_id, 10)
        .await
        .context("list runs after complete")?;
    assert_eq!(runs.len(), expected);
    if expected > 0 {
        assert!(matches!(runs[0].status, RunStatus::Ok));
    }
    Ok(())
}

async fn delete_routine_and_assert_absent(
    db: &Arc<dyn Database>,
    routine_id: uuid::Uuid,
) -> Result<()> {
    let deleted = db.delete_routine(routine_id).await.context("delete")?;
    assert!(deleted);

    // Delete non-existent
    let deleted = db
        .delete_routine(routine_id)
        .await
        .context("delete again")?;
    assert!(!deleted);
    Ok(())
}

#[tokio::test]
async fn test_routine_crud() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let routine_id = create_routine_fixture(db)
        .await
        .expect("create routine fixture");
    let run_id = start_routine_run(db, routine_id)
        .await
        .expect("start routine run");
    complete_routine_run_ok(db, run_id)
        .await
        .expect("complete routine run");
    assert_history_len(db, routine_id, 1)
        .await
        .expect("assert history length");
    delete_routine_and_assert_absent(db, routine_id)
        .await
        .expect("delete routine");
}

#[tokio::test]
async fn test_routine_runtime_update() {
    use crate::agent::routine::{NotifyConfig, Routine, RoutineAction, RoutineGuardrails, Trigger};

    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let routine_id = uuid::Uuid::new_v4();
    let routine = Routine {
        id: routine_id,
        name: "runtime-test".to_string(),
        description: "Test runtime update".to_string(),
        user_id: "user1".to_string(),
        enabled: true,
        trigger: Trigger::Manual,
        action: RoutineAction::Lightweight {
            prompt: "test".to_string(),
            context_paths: vec![],
            max_tokens: 100,
        },
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(0),
            max_concurrent: 1,
            dedup_window: None,
        },
        notify: NotifyConfig {
            channel: None,
            user: "user1".to_string(),
            on_attention: false,
            on_failure: false,
            on_success: false,
        },
        last_run_at: None,
        next_fire_at: None,
        run_count: 0,
        consecutive_failures: 0,
        state: serde_json::json!({}),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db.create_routine(&routine).await.expect("create");

    let now = chrono::Utc::now();
    db.update_routine_runtime(RoutineRuntimeUpdate {
        id: routine_id,
        last_run_at: now,
        next_fire_at: Some(now + chrono::TimeDelta::seconds(3600)),
        run_count: 5,
        consecutive_failures: 2,
        state: &serde_json::json!({"last_result": "ok"}),
    })
    .await
    .expect("update runtime");

    let fetched = db
        .get_routine(routine_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(fetched.run_count, 5);
    assert_eq!(fetched.consecutive_failures, 2);
    assert!(fetched.last_run_at.is_some());
    assert!(fetched.next_fire_at.is_some());

    // Cleanup
    db.delete_routine(routine_id).await.expect("delete");
}
