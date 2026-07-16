//! Sandbox job lifecycle, mode, and job-event persistence tests.

use crate::db::SandboxJobStatusUpdate;
use crate::testing::TestHarnessBuilder;

#[tokio::test]
async fn test_sandbox_job_lifecycle() {
    use crate::history::SandboxJobRecord;

    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let job_id = uuid::Uuid::new_v4();
    let job = SandboxJobRecord {
        id: job_id,
        task: "Build a test tool".to_string(),
        status: "creating".to_string(),
        user_id: crate::db::UserId::from("user1"),
        project_dir: "/workspace/test".to_string(),
        success: None,
        failure_reason: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        completed_at: None,
        credential_grants_json: "[]".to_string(),
    };

    // Create
    db.save_sandbox_job(&job).await.expect("save sandbox job");

    // Get
    let fetched = db
        .get_sandbox_job(job_id)
        .await
        .expect("get")
        .expect("should exist");
    assert_eq!(fetched.task, "Build a test tool");
    assert_eq!(fetched.status, "creating");

    // Update status to running
    db.update_sandbox_job_status(SandboxJobStatusUpdate {
        id: job_id,
        status: crate::db::SandboxJobStatus::from("running"),
        success: None,
        message: None,
        started_at: Some(chrono::Utc::now()),
        completed_at: None,
    })
    .await
    .expect("update to running");

    // Update to completed
    db.update_sandbox_job_status(SandboxJobStatusUpdate {
        id: job_id,
        status: crate::db::SandboxJobStatus::from("completed"),
        success: Some(true),
        message: Some("Done"),
        started_at: None,
        completed_at: Some(chrono::Utc::now()),
    })
    .await
    .expect("update to completed");

    let fetched = db
        .get_sandbox_job(job_id)
        .await
        .expect("get")
        .expect("should exist");
    assert_eq!(fetched.status, "completed");
    assert_eq!(fetched.success, Some(true));

    // List
    let all = db.list_sandbox_jobs().await.expect("list");
    assert!(!all.is_empty());

    // Summary
    let summary = db.sandbox_job_summary().await.expect("summary");
    assert!(summary.total >= 1);

    // Per-user list
    let user_jobs = db
        .list_sandbox_jobs_for_user(crate::db::UserId::from("user1"))
        .await
        .expect("user list");
    assert!(!user_jobs.is_empty());

    // Ownership check
    let belongs = db
        .sandbox_job_belongs_to_user(job_id, crate::db::UserId::from("user1"))
        .await
        .expect("belongs check");
    assert!(belongs);
    let not_belongs = db
        .sandbox_job_belongs_to_user(job_id, crate::db::UserId::from("other_user"))
        .await
        .expect("belongs check");
    assert!(!not_belongs);
}

#[tokio::test]
async fn test_sandbox_job_mode() {
    use crate::history::SandboxJobRecord;

    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let job_id = uuid::Uuid::new_v4();
    let job = SandboxJobRecord {
        id: job_id,
        task: "Mode test".to_string(),
        status: "creating".to_string(),
        user_id: crate::db::UserId::from("user1"),
        project_dir: "/workspace".to_string(),
        success: None,
        failure_reason: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        completed_at: None,
        credential_grants_json: "[]".to_string(),
    };
    db.save_sandbox_job(&job).await.expect("save");

    // Default mode
    let mode = db.get_sandbox_job_mode(job_id).await.expect("get mode");
    // Default is "worker" per schema or NULL
    assert!(mode.is_none() || mode == Some(crate::db::SandboxMode::Worker));

    // Update mode
    db.update_sandbox_job_mode(job_id, crate::db::SandboxMode::ClaudeCode)
        .await
        .expect("update mode");
    let mode = db
        .get_sandbox_job_mode(job_id)
        .await
        .expect("get mode")
        .expect("should have mode");
    assert_eq!(mode, crate::db::SandboxMode::ClaudeCode);
}

#[tokio::test]
async fn test_job_events() {
    use crate::history::SandboxJobRecord;

    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    // Create a sandbox job first (foreign key)
    let job_id = uuid::Uuid::new_v4();
    let job = SandboxJobRecord {
        id: job_id,
        task: "Event test".to_string(),
        status: "running".to_string(),
        user_id: crate::db::UserId::from("user1"),
        project_dir: "/workspace".to_string(),
        success: None,
        failure_reason: None,
        created_at: chrono::Utc::now(),
        started_at: Some(chrono::Utc::now()),
        completed_at: None,
        credential_grants_json: "[]".to_string(),
    };
    db.save_sandbox_job(&job).await.expect("save job");

    // Save events
    db.save_job_event(
        job_id,
        crate::db::SandboxEventType::from("tool_call"),
        &serde_json::json!({"tool": "shell", "args": {"command": "ls"}}),
    )
    .await
    .expect("save event 1");

    db.save_job_event(
        job_id,
        crate::db::SandboxEventType::from("tool_result"),
        &serde_json::json!({"output": "file1.txt\nfile2.txt"}),
    )
    .await
    .expect("save event 2");

    db.save_job_event(
        job_id,
        crate::db::SandboxEventType::from("llm_response"),
        &serde_json::json!({"content": "Found 2 files"}),
    )
    .await
    .expect("save event 3");

    // List all events
    let events = db
        .list_job_events(job_id, None, None)
        .await
        .expect("list events");
    assert_eq!(events.len(), 3);

    // List with limit
    let events = db
        .list_job_events(job_id, None, Some(2))
        .await
        .expect("list events limited");
    assert_eq!(events.len(), 2);

    // List older events before the oldest event in the limited page.
    let events = db
        .list_job_events(job_id, Some(events[0].id), Some(2))
        .await
        .expect("list events before cursor");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "tool_call");
}
