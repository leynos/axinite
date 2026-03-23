//! Unit coverage for orphan-state decisions, label parsing, and threshold
//! handling in the sandbox reaper.

use super::*;
use std::collections::HashMap;

use crate::context::{JobContext, JobState};
use anyhow::{Context as _, Result};
use rstest::rstest;

/// Build a standard 600-second-threshold `ReaperConfig`, fabricate a single
/// `ironclaw.created_at` label `age_offset` before now, parse it, and return
/// whether the container is past the orphan threshold.
fn container_age_is_past_threshold(age_offset: chrono::Duration) -> bool {
    let cfg = ReaperConfig {
        orphan_threshold: Duration::from_secs(600),
        ..Default::default()
    };
    let now = Utc::now();
    let mut labels = HashMap::new();
    labels.insert(
        "ironclaw.created_at".to_string(),
        (now - age_offset).to_rfc3339(),
    );
    let created_at = parse_created_at_label(&labels, None)
        .expect("timestamp should parse in orphan-threshold test");
    is_past_orphan_threshold(created_at, &cfg, now)
}

#[test]
fn orphan_threshold_filters_young_containers() {
    assert!(
        !container_age_is_past_threshold(chrono::Duration::minutes(2)),
        "Young container should be skipped"
    );
}

#[test]
fn orphan_threshold_allows_old_containers() {
    assert!(
        container_age_is_past_threshold(chrono::Duration::minutes(15)),
        "Old container should be reaped"
    );
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

/// Create a freshly-pending job and return whether its state `is_active()`.
///
/// `tag` is embedded verbatim in error context so that failure messages match
/// the original per-test strings.
async fn create_active_job(ctx_mgr: &ContextManager, description: &str, tag: &str) -> Result<bool> {
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", description)
        .await
        .with_context(|| format!("create_job_for_user failed for {tag}"))?;
    let is_active = ctx_mgr
        .get_context(job_id)
        .await
        .with_context(|| format!("get_context failed for {tag} job_id"))?
        .state
        .is_active();
    Ok(is_active)
}

#[rstest]
#[case(
    "test description",
    "active_job_is_not_orphaned",
    "Pending job should be active"
)]
#[case(
    "test job",
    "active_job_prevents_cleanup_of_old_container",
    "Active job should prevent cleanup"
)]
#[tokio::test]
async fn active_job_remains_active(
    #[case] description: &str,
    #[case] tag: &str,
    #[case] assertion_message: &str,
) -> Result<()> {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    assert!(
        create_active_job(&ctx_mgr, description, tag).await?,
        "{}",
        assertion_message
    );
    Ok(())
}

#[rstest]
#[case(JobState::Failed)]
#[case(JobState::Cancelled)]
#[tokio::test]
async fn terminal_job_is_treated_as_orphaned(#[case] state: JobState) {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let (_job_id, ctx) = make_terminal_job(&ctx_mgr, "test description", state).await;
    assert!(
        !ctx.state.is_active(),
        "Terminal job should be treated as orphaned"
    );
}

#[test]
fn parse_container_labels_extracts_job_id_and_timestamp() {
    let mut labels = HashMap::new();
    let job_id = Uuid::new_v4();
    let created_at_raw = "2024-01-15T10:30:45+00:00";
    labels.insert("ironclaw.job_id".to_string(), job_id.to_string());
    labels.insert(
        "ironclaw.created_at".to_string(),
        created_at_raw.to_string(),
    );

    let parsed_id = parse_job_id_label(&labels, "ironclaw.job_id");
    assert_eq!(parsed_id, Some(job_id));

    let parsed_time =
        parse_created_at_label(&labels, None).expect("expected created_at label to parse");
    let expected_time = chrono::DateTime::parse_from_rfc3339(created_at_raw)
        .expect("expected created_at label fixture to be valid RFC3339")
        .with_timezone(&Utc);
    assert_eq!(parsed_time, expected_time);
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
    let expected_fallback = chrono::DateTime::<Utc>::from_timestamp(1705324245, 0)
        .expect("expected fallback timestamp fixture to be valid");
    assert_eq!(
        fallback,
        Some(expected_fallback),
        "Docker timestamp fallback should parse successfully"
    );
}

#[test]
fn age_calculation_correctly_filters_containers() {
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

/// Create a job, optionally transition it to `new_state`, then return whether
/// `ctx.state.is_active()` is `true` for the resulting context.
///
/// `tag` is embedded verbatim in error context so that failures match the
/// original per-job error messages.
async fn create_job_in_state(
    ctx_mgr: &ContextManager,
    tag: &str,
    description: &str,
    new_state: Option<crate::context::JobState>,
) -> Result<bool> {
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", description)
        .await
        .with_context(|| format!("create_job_for_user failed for {tag}"))?;
    if let Some(state) = new_state {
        ctx_mgr
            .update_context(job_id, |ctx| {
                ctx.state = state;
            })
            .await
            .with_context(|| format!("update_context failed when setting state for {tag}"))?;
    }
    let is_active = ctx_mgr
        .get_context(job_id)
        .await
        .with_context(|| format!("get_context failed for {tag}"))?
        .state
        .is_active();
    Ok(is_active)
}

#[rstest]
#[case(None, true, "pending_job")]
#[case(Some(JobState::InProgress), true, "in_progress_job")]
#[case(Some(JobState::Completed), true, "completed_job")]
#[case(Some(JobState::Failed), false, "failed_job")]
#[case(Some(JobState::Cancelled), false, "cancelled_job")]
#[tokio::test]
async fn reaper_cleanup_decision_matrix(
    #[case] state: Option<JobState>,
    #[case] expected_active: bool,
    #[case] name: &str,
) -> Result<()> {
    let ctx_mgr = Arc::new(ContextManager::new(5));

    let result = create_job_in_state(&ctx_mgr, name, "test", state).await?;
    assert_eq!(
        result, expected_active,
        "Job state {:?} should have is_active() == {}",
        state, expected_active
    );

    Ok(())
}

#[tokio::test]
async fn missing_job_is_treated_as_inactive() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let missing_job = Uuid::new_v4();
    let is_active = match ctx_mgr.get_context(missing_job).await {
        Ok(ctx) => ctx.state.is_active(),
        Err(_) => false,
    };
    assert!(!is_active, "Missing job should be treated as inactive");
}

#[cfg(all(test, feature = "docker", not(target_env = "msvc")))]
mod e2e;
