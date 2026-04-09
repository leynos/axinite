//! Tests for DefaultSelfRepair.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rstest::rstest;

use crate::agent::self_repair::default::{DefaultSelfRepair, duration_since};
use crate::agent::self_repair::{BrokenTool, NativeSelfRepair, RepairResult, StuckJob};
use crate::context::{ContextManager, JobState};
use uuid::Uuid;

// === QA Plan - Self-repair stuck job tests ===

#[tokio::test]
async fn detect_no_stuck_jobs_when_all_healthy() {
    let cm = Arc::new(ContextManager::new(10));

    // Create a job and leave it Pending (not stuck).
    cm.create_job("Job 1", "desc")
        .await
        .expect("create job in detect_no_stuck_jobs_when_all_healthy");

    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3);
    let stuck = NativeSelfRepair::detect_stuck_jobs(&repair).await;
    assert!(stuck.is_empty());
}

#[tokio::test]
async fn detect_stuck_job_finds_stuck_state() {
    let cm = Arc::new(ContextManager::new(10));
    let job_id = cm
        .create_job("Stuck job", "desc")
        .await
        .expect("create job in detect_stuck_job_finds_stuck_state");

    // Transition to InProgress, then to Stuck.
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("transition to InProgress for job")
        .expect("transition to InProgress succeeded");
    cm.update_context(job_id, |ctx| {
        ctx.transition_to(JobState::Stuck, Some("timed out".to_string()))
    })
    .await
    .expect("transition to Stuck for job")
    .expect("transition to Stuck succeeded");

    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(0), 3);
    let stuck = NativeSelfRepair::detect_stuck_jobs(&repair).await;
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].job_id, job_id);
}

// Shared helper for these tests.
async fn seed_job_with_two_stuck_transitions(
    cm: &Arc<ContextManager>,
    first_age_secs: i64,
    second_age_secs: i64,
) -> Uuid {
    let job_id = cm
        .create_job("Stuck job", "desc")
        .await
        .expect("failed to await create_job");

    // First transition: InProgress -> Stuck, then backdate timestamp by first_age_secs
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to await update_context")
        .expect("expected in-progress transition to succeed");
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::Stuck, None))
        .await
        .expect("failed to await update_context")
        .expect("expected first stuck transition to succeed");
    cm.update_context(job_id, |ctx| {
        let stuck_since = Utc::now() - chrono::Duration::seconds(first_age_secs);
        let Some(last_transition) = ctx.transitions.last_mut() else {
            return Err("missing stuck transition".to_string());
        };
        last_transition.timestamp = stuck_since;
        Ok(())
    })
    .await
    .expect("failed to await update_context")
    .expect("expected first stuck timestamp update to succeed");

    // Second transition: Stuck -> InProgress -> Stuck, then backdate timestamp by second_age_secs
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to await update_context")
        .expect("expected recovery transition to succeed");
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::Stuck, None))
        .await
        .expect("failed to await update_context")
        .expect("expected second stuck transition to succeed");
    cm.update_context(job_id, |ctx| {
        let stuck_since = Utc::now() - chrono::Duration::seconds(second_age_secs);
        let Some(last_transition) = ctx.transitions.last_mut() else {
            return Err("missing stuck transition".to_string());
        };
        last_transition.timestamp = stuck_since;
        Ok(())
    })
    .await
    .expect("failed to await update_context")
    .expect("expected second stuck timestamp update to succeed");

    job_id
}

#[rstest]
#[case(59, false)]
#[case(60, true)]
#[case(61, true)]
#[case(120, true)]
#[tokio::test]
async fn detect_stuck_jobs_uses_latest_stuck_transition(
    #[case] second_age_secs: i64,
    #[case] should_detect: bool,
) {
    let cm = Arc::new(ContextManager::new(10));
    let threshold = Duration::from_secs(60);

    // First transition is always 120s old, second transition age varies by test case.
    let job_id = seed_job_with_two_stuck_transitions(&cm, 120, second_age_secs).await;
    let repair = DefaultSelfRepair::new(Arc::clone(&cm), threshold, 3);

    let stuck_jobs = NativeSelfRepair::detect_stuck_jobs(&repair).await;

    if should_detect {
        assert_eq!(
            stuck_jobs.len(),
            1,
            "expected exactly one detected stuck job when second transition is {second_age_secs}s old"
        );

        let ctx = cm
            .get_context(job_id)
            .await
            .expect("context should exist after transitions");
        let latest_stuck_since = ctx
            .stuck_since()
            .expect("stuck_since should be set after Stuck transition");

        assert_eq!(
            stuck_jobs[0].stuck_since, latest_stuck_since,
            "detection must use the latest Stuck transition timestamp"
        );
        assert!(
            stuck_jobs[0].stuck_duration >= threshold,
            "stuck_duration must reflect time since latest Stuck >= threshold"
        );
    } else {
        assert!(
            stuck_jobs.is_empty(),
            "job {job_id} should not be detected while latest Stuck ({second_age_secs}s) < threshold"
        );
    }
}

#[tokio::test]
async fn repair_stuck_job_succeeds_within_limit() {
    let cm = Arc::new(ContextManager::new(10));
    let job_id = cm
        .create_job("Repairable", "desc")
        .await
        .expect("failed to create job");

    // Move to InProgress -> Stuck.
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to transition to InProgress")
        .expect("transition to InProgress failed");
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::Stuck, None))
        .await
        .expect("failed to transition to Stuck")
        .expect("transition to Stuck failed");

    let repair = DefaultSelfRepair::new(Arc::clone(&cm), Duration::from_secs(60), 3);

    let stuck_job = StuckJob {
        job_id,
        stuck_since: Utc::now(),
        stuck_duration: Duration::from_secs(120),
        last_error: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_stuck_job(&repair, &stuck_job)
        .await
        .expect("repair_stuck_job failed");
    assert!(
        matches!(result, RepairResult::Success { .. }),
        "Expected Success, got: {:?}",
        result
    );

    // Job should be back to InProgress after recovery.
    let ctx = cm.get_context(job_id).await.expect("failed to get context");
    assert_eq!(ctx.state, JobState::InProgress);
}

#[tokio::test]
async fn repair_stuck_job_returns_manual_when_limit_exceeded() {
    let cm = Arc::new(ContextManager::new(10));
    let job_id = cm
        .create_job("Unrepairable", "desc")
        .await
        .expect("create_job failed in repair_stuck_job_returns_manual_when_limit_exceeded");

    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 2);

    let stuck_job = StuckJob {
        job_id,
        stuck_since: Utc::now(),
        stuck_duration: Duration::from_secs(300),
        last_error: Some("persistent failure".to_string()),
        repair_attempts: 2, // == max
    };

    let result = NativeSelfRepair::repair_stuck_job(&repair, &stuck_job)
        .await
        .expect("repair_stuck_job failed in repair_stuck_job_returns_manual_when_limit_exceeded");
    assert!(
        matches!(result, RepairResult::ManualRequired { .. }),
        "Expected ManualRequired, got: {:?}",
        result
    );
}

#[tokio::test]
async fn repair_stuck_job_returns_success_when_already_recovered() {
    let cm = Arc::new(ContextManager::new(10));
    let job_id = cm
        .create_job("AlreadyRecovered", "desc")
        .await
        .expect("create_job failed in repair_stuck_job_returns_success_when_already_recovered");

    // Move to InProgress -> Stuck -> InProgress (recovered without repair)
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to transition to InProgress")
        .expect("transition to InProgress failed");
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::Stuck, None))
        .await
        .expect("failed to transition to Stuck")
        .expect("transition to Stuck failed");
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to transition to InProgress")
        .expect("transition to InProgress failed");

    let repair = DefaultSelfRepair::new(Arc::clone(&cm), Duration::from_secs(60), 3);

    let stuck_job = StuckJob {
        job_id,
        stuck_since: Utc::now(),
        stuck_duration: Duration::from_secs(120),
        last_error: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_stuck_job(&repair, &stuck_job)
        .await
        .expect(
            "repair_stuck_job failed in repair_stuck_job_returns_success_when_already_recovered",
        );

    // Should return Success (no-op) because job is already recovered
    assert!(
        matches!(result, RepairResult::Success { .. }),
        "Expected Success when job already recovered, got: {:?}",
        result
    );
}

#[tokio::test]
async fn detect_broken_tools_returns_empty_without_store() {
    let cm = Arc::new(ContextManager::new(10));
    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3);

    // No store configured, should return empty.
    let broken = NativeSelfRepair::detect_broken_tools(&repair).await;
    assert!(broken.is_empty());
}

#[tokio::test]
async fn repair_broken_tool_returns_manual_without_builder() {
    let cm = Arc::new(ContextManager::new(10));
    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3);

    let broken = BrokenTool {
        name: "test-tool".to_string(),
        failure_count: 10,
        last_error: Some("crash".to_string()),
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_broken_tool(&repair, &broken)
        .await
        .expect("repair_broken_tool failed in repair_broken_tool_returns_manual_without_builder");
    assert!(
        matches!(result, RepairResult::ManualRequired { .. }),
        "Expected ManualRequired without builder, got: {:?}",
        result
    );
}

#[test]
fn duration_since_millisecond_precision() {
    use chrono::Duration as ChronoDuration;

    let now = Utc::now();
    let start = now - ChronoDuration::milliseconds(500);
    let elapsed = duration_since(now, start);

    // Should be >= 500ms and < 1s (proving millisecond resolution, not second)
    assert!(
        elapsed >= Duration::from_millis(500),
        "Expected >= 500ms, got {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "Expected < 1s, got {:?}",
        elapsed
    );
}
