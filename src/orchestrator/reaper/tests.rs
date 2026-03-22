//! Unit coverage for orphan-state decisions, label parsing, and threshold
//! handling in the sandbox reaper.

use super::*;
use std::collections::HashMap;

use crate::context::{JobContext, JobState};

#[test]
fn orphan_threshold_filters_young_containers() {
    let cfg = ReaperConfig {
        orphan_threshold: Duration::from_secs(600),
        ..Default::default()
    };
    let now = Utc::now();
    let mut labels = HashMap::new();
    labels.insert(
        "ironclaw.created_at".to_string(),
        (now - chrono::Duration::minutes(2)).to_rfc3339(),
    );
    let created_at = parse_created_at_label(&labels, None)
        .expect("expected young container timestamp to parse in threshold test");
    assert!(
        !is_past_orphan_threshold(created_at, &cfg, now),
        "Young container should be skipped"
    );
}

#[test]
fn orphan_threshold_allows_old_containers() {
    let cfg = ReaperConfig {
        orphan_threshold: Duration::from_secs(600),
        ..Default::default()
    };
    let now = Utc::now();
    let mut labels = HashMap::new();
    labels.insert(
        "ironclaw.created_at".to_string(),
        (now - chrono::Duration::minutes(15)).to_rfc3339(),
    );
    let created_at =
        parse_created_at_label(&labels, None).expect("expected old container timestamp to parse");
    assert!(
        is_past_orphan_threshold(created_at, &cfg, now),
        "Old container should be reaped"
    );
}

#[tokio::test]
async fn active_job_is_not_orphaned() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", "test description")
        .await
        .expect("create_job_for_user failed for active_job_is_not_orphaned");

    let ctx = ctx_mgr
        .get_context(job_id)
        .await
        .expect("get_context failed for active_job_is_not_orphaned job_id");
    assert!(ctx.state.is_active(), "Pending job should be active");
}

#[tokio::test]
async fn missing_job_is_treated_as_orphaned() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let job_id = Uuid::new_v4();
    let is_active = match ctx_mgr.get_context(job_id).await {
        Ok(ctx) => ctx.state.is_active(),
        Err(_) => false,
    };
    assert!(!is_active, "Missing job should be treated as orphaned");
}

async fn make_terminal_job(
    ctx_mgr: &ContextManager,
    description: &str,
    state: JobState,
) -> (Uuid, JobContext) {
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", description)
        .await
        .expect("create_job_for_user failed in make_terminal_job");
    ctx_mgr
        .update_context(job_id, |ctx| {
            ctx.state = state;
        })
        .await
        .expect("update_context failed when setting terminal JobState in make_terminal_job");
    let ctx = ctx_mgr
        .get_context(job_id)
        .await
        .expect("get_context failed in make_terminal_job");
    (job_id, ctx)
}

#[tokio::test]
async fn terminal_job_is_treated_as_orphaned() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let (_job_id, ctx) = make_terminal_job(&ctx_mgr, "test description", JobState::Failed).await;
    assert!(
        !ctx.state.is_active(),
        "Failed job should be treated as orphaned"
    );
}

#[test]
fn parse_container_labels_extracts_job_id_and_timestamp() {
    let mut labels = HashMap::new();
    let job_id = Uuid::new_v4();
    labels.insert("ironclaw.job_id".to_string(), job_id.to_string());
    labels.insert(
        "ironclaw.created_at".to_string(),
        "2024-01-15T10:30:45+00:00".to_string(),
    );

    let parsed_id = parse_job_id_label(&labels, "ironclaw.job_id");
    assert_eq!(parsed_id, Some(job_id));

    let parsed_time = parse_created_at_label(&labels, None);
    assert!(parsed_time.is_some());
}

#[test]
fn missing_job_id_label_is_skipped() {
    let labels: HashMap<String, String> = HashMap::new();
    let job_id = parse_job_id_label(&labels, "ironclaw.job_id");
    assert_eq!(job_id, None);
}

#[test]
fn malformed_timestamp_fallback_works() {
    let mut labels: HashMap<String, String> = HashMap::new();
    labels.insert(
        "ironclaw.created_at".to_string(),
        "invalid-date".to_string(),
    );

    let parsed_time = parse_created_at_label(&labels, None);
    assert!(
        parsed_time.is_none(),
        "Malformed timestamp should fail to parse"
    );

    let fallback = parse_created_at_label(&labels, Some(1705324245));
    assert!(
        fallback.is_some(),
        "Docker timestamp fallback should parse successfully"
    );
}

#[tokio::test]
async fn age_calculation_correctly_filters_containers() {
    let cfg = ReaperConfig {
        orphan_threshold: Duration::from_secs(600),
        ..Default::default()
    };
    let now = Utc::now();
    let young_container = now - chrono::Duration::minutes(2);
    let old_container = now - chrono::Duration::minutes(20);

    assert!(
        !is_past_orphan_threshold(young_container, &cfg, now),
        "Young container should not be cleaned"
    );
    assert!(
        is_past_orphan_threshold(old_container, &cfg, now),
        "Old container should be cleaned"
    );
}

#[tokio::test]
async fn active_job_prevents_cleanup_of_old_container() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", "test job")
        .await
        .expect("create_job_for_user failed for active_job_prevents_cleanup_of_old_container");

    let ctx = ctx_mgr
        .get_context(job_id)
        .await
        .expect("get_context failed for active_job_prevents_cleanup_of_old_container job_id");
    assert!(ctx.state.is_active());

    let is_active = match ctx_mgr.get_context(job_id).await {
        Ok(ctx) => ctx.state.is_active(),
        Err(_) => false,
    };
    assert!(is_active, "Active job should prevent cleanup");
}

#[tokio::test]
async fn failed_job_allows_cleanup() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let (_job_id, ctx) = make_terminal_job(&ctx_mgr, "test", JobState::Cancelled).await;
    assert!(
        !ctx.state.is_active(),
        "Failed job (terminal state) should allow cleanup"
    );
}

#[test]
fn reaper_config_defaults_are_reasonable() {
    let cfg = ReaperConfig::default();
    assert_eq!(cfg.scan_interval, Duration::from_secs(300));
    assert_eq!(cfg.orphan_threshold, Duration::from_secs(600));
    assert_eq!(cfg.container_label, "ironclaw.job_id");
}

#[test]
fn reaper_config_can_be_customized() {
    let cfg = ReaperConfig {
        scan_interval: Duration::from_secs(60),
        orphan_threshold: Duration::from_secs(300),
        container_label: "custom.label".to_string(),
    };
    assert_eq!(cfg.scan_interval, Duration::from_secs(60));
    assert_eq!(cfg.orphan_threshold, Duration::from_secs(300));
    assert_eq!(cfg.container_label, "custom.label");
}

#[tokio::test]
async fn reaper_cleanup_decision_matrix() {
    let ctx_mgr = Arc::new(ContextManager::new(5));

    let job1 = ctx_mgr
        .create_job_for_user("default", "test", "test1")
        .await
        .expect("create_job_for_user failed for reaper_cleanup_decision_matrix job1");
    let ctx1 = ctx_mgr
        .get_context(job1)
        .await
        .expect("get_context failed for reaper_cleanup_decision_matrix job1");
    assert!(ctx1.state.is_active(), "Pending job is active");

    let job2 = ctx_mgr
        .create_job_for_user("default", "test", "test2")
        .await
        .expect("create_job_for_user failed for reaper_cleanup_decision_matrix job2");
    ctx_mgr
        .update_context(job2, |ctx| {
            ctx.state = JobState::InProgress;
        })
        .await
        .expect("update_context failed when setting JobState::InProgress for job2");
    let ctx2 = ctx_mgr
        .get_context(job2)
        .await
        .expect("get_context failed for reaper_cleanup_decision_matrix job2");
    assert!(ctx2.state.is_active(), "InProgress job is active");

    let job3 = ctx_mgr
        .create_job_for_user("default", "test", "test3")
        .await
        .expect("create_job_for_user failed for reaper_cleanup_decision_matrix job3");
    ctx_mgr
        .update_context(job3, |ctx| {
            ctx.state = JobState::Completed;
        })
        .await
        .expect("update_context failed when setting JobState::Completed for job3");
    let ctx3 = ctx_mgr
        .get_context(job3)
        .await
        .expect("get_context failed for reaper_cleanup_decision_matrix job3");
    assert!(ctx3.state.is_active(), "Completed is still active");

    let job4 = ctx_mgr
        .create_job_for_user("default", "test", "test4")
        .await
        .expect("create_job_for_user failed for reaper_cleanup_decision_matrix job4");
    ctx_mgr
        .update_context(job4, |ctx| {
            ctx.state = JobState::Failed;
        })
        .await
        .expect("update_context failed when setting JobState::Failed for job4");
    let ctx4 = ctx_mgr
        .get_context(job4)
        .await
        .expect("get_context failed for reaper_cleanup_decision_matrix job4");
    assert!(!ctx4.state.is_active(), "Failed job is terminal");

    let job5 = ctx_mgr
        .create_job_for_user("default", "test", "test5")
        .await
        .expect("create_job_for_user failed for reaper_cleanup_decision_matrix job5");
    ctx_mgr
        .update_context(job5, |ctx| {
            ctx.state = JobState::Cancelled;
        })
        .await
        .expect("update_context failed when setting JobState::Cancelled for job5");
    let ctx5 = ctx_mgr
        .get_context(job5)
        .await
        .expect("get_context failed for reaper_cleanup_decision_matrix job5");
    assert!(!ctx5.state.is_active(), "Cancelled job is terminal");

    let missing_job = Uuid::new_v4();
    let is_active = match ctx_mgr.get_context(missing_job).await {
        Ok(ctx) => ctx.state.is_active(),
        Err(_) => false,
    };
    assert!(!is_active, "Missing job should be treated as inactive");
}

#[cfg(all(test, feature = "docker", not(target_env = "msvc")))]
mod e2e;
