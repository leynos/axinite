//! Job, tool-failure, LLM-call, and estimation persistence tests.

use rust_decimal::Decimal;

use crate::db::{EstimationActualsParams, EstimationSnapshotParams};
use crate::testing::TestHarnessBuilder;

#[tokio::test]
async fn test_job_action_persistence() {
    use crate::context::{ActionRecord, JobContext, JobState};

    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let ctx = JobContext::with_user("user1", "Do something", "test task");

    let job_id = ctx.job_id;

    // Save job.
    db.save_job(&ctx).await.expect("save job");

    // Get job back.
    let fetched = db.get_job(job_id).await.expect("get job");
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.job_id, job_id);

    // Save an action.
    let action = ActionRecord {
        id: uuid::Uuid::new_v4(),
        sequence: 1,
        tool_name: "echo".to_string(),
        input: serde_json::json!({"message": "hello"}),
        output_raw: Some("hello".to_string()),
        output_sanitized: None,
        sanitization_warnings: vec![],
        cost: None,
        duration: std::time::Duration::from_millis(42),
        success: true,
        error: None,
        executed_at: chrono::Utc::now(),
    };
    db.save_action(job_id, &action).await.expect("save action");

    // Retrieve actions.
    let actions = db.get_job_actions(job_id).await.expect("get actions");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].tool_name, "echo");
    assert_eq!(actions[0].output_raw, Some("hello".to_string()));
    assert!(actions[0].success);
    assert_eq!(actions[0].duration, std::time::Duration::from_millis(42));

    // Update job status.
    db.update_job_status(job_id, JobState::Completed, None)
        .await
        .expect("update status");

    let updated = db
        .get_job(job_id)
        .await
        .expect("get updated job")
        .expect("job should exist");
    assert!(matches!(updated.state, JobState::Completed));
}

#[tokio::test]
async fn test_tool_failure_tracking() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    // Record some failures
    db.record_tool_failure("bad_tool", "connection refused")
        .await
        .expect("record 1");
    db.record_tool_failure("bad_tool", "timeout")
        .await
        .expect("record 2");
    db.record_tool_failure("bad_tool", "parse error")
        .await
        .expect("record 3");

    // Get broken tools (threshold = 2, should include bad_tool with 3 failures)
    let broken = db.get_broken_tools(2).await.expect("get broken");
    assert!(!broken.is_empty());
    let found = broken.iter().find(|b| b.name == "bad_tool");
    assert!(found.is_some(), "bad_tool should be in broken tools list");

    // Mark as repaired
    db.mark_tool_repaired("bad_tool")
        .await
        .expect("mark repaired");
}

#[tokio::test]
async fn test_llm_call_recording() {
    use crate::history::LlmCallRecord;

    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let record = LlmCallRecord {
        job_id: None,
        conversation_id: None,
        provider: "openai",
        model: "gpt-4",
        input_tokens: 100,
        output_tokens: 50,
        cost: Decimal::new(5, 3), // 0.005
        purpose: Some("test"),
    };

    let call_id = db.record_llm_call(&record).await.expect("record llm call");
    assert!(!call_id.is_nil());
}

#[tokio::test]
async fn test_estimation_snapshot_round_trip() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    // Create a job first
    let job_ctx = crate::context::JobContext::with_user("user1", "Estimate test", "testing");
    let job_id = job_ctx.job_id;
    db.save_job(&job_ctx).await.expect("save job");

    // Save estimation snapshot
    let snap_id = db
        .save_estimation_snapshot(EstimationSnapshotParams {
            job_id,
            category: "code_generation",
            tool_names: &["shell".to_string(), "write_file".to_string()],
            estimated_cost: Decimal::new(50, 2), // 0.50
            estimated_time_secs: 120,
            estimated_value: Decimal::new(500, 2), // 5.00
        })
        .await
        .expect("save snapshot");
    assert!(!snap_id.is_nil());

    // Update with actuals
    db.update_estimation_actuals(EstimationActualsParams {
        id: snap_id,
        actual_cost: Decimal::new(45, 2), // 0.45
        actual_time_secs: 110,
        actual_value: Some(Decimal::new(600, 2)), // 6.00
    })
    .await
    .expect("update actuals");
}
