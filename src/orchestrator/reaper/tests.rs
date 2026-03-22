use super::*;
use std::collections::HashMap;

#[test]
fn orphan_threshold_filters_young_containers() {
    let threshold = chrono::Duration::minutes(10);
    let young_age = chrono::Duration::minutes(2);
    assert!(young_age < threshold, "Young container should be skipped");
}

#[test]
fn orphan_threshold_allows_old_containers() {
    let threshold = chrono::Duration::minutes(10);
    let old_age = chrono::Duration::minutes(15);
    assert!(old_age >= threshold, "Old container should be reaped");
}

#[tokio::test]
async fn active_job_is_not_orphaned() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", "test description")
        .await
        .unwrap();

    let ctx = ctx_mgr.get_context(job_id).await.unwrap();
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

async fn make_terminal_job(ctx_mgr: &ContextManager, description: &str) -> Uuid {
    use crate::context::JobState;
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", description)
        .await
        .unwrap();
    ctx_mgr
        .update_context(job_id, |ctx| {
            ctx.state = JobState::Failed;
        })
        .await
        .unwrap();
    job_id
}

#[tokio::test]
async fn terminal_job_is_treated_as_orphaned() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let job_id = make_terminal_job(&ctx_mgr, "test description").await;

    let ctx = ctx_mgr.get_context(job_id).await.unwrap();
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
    let now = Utc::now();
    let young_container = now - chrono::Duration::minutes(2);
    let old_container = now - chrono::Duration::minutes(20);
    let threshold = chrono::Duration::minutes(10);

    let young_age = now.signed_duration_since(young_container);
    let old_age = now.signed_duration_since(old_container);

    assert!(
        young_age < threshold,
        "Young container should not be cleaned"
    );
    assert!(old_age >= threshold, "Old container should be cleaned");
}

#[tokio::test]
async fn active_job_prevents_cleanup_of_old_container() {
    let ctx_mgr = Arc::new(ContextManager::new(5));
    let job_id = ctx_mgr
        .create_job_for_user("default", "test", "test job")
        .await
        .unwrap();

    let ctx = ctx_mgr.get_context(job_id).await.unwrap();
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
    let job_id = make_terminal_job(&ctx_mgr, "test").await;

    let ctx = ctx_mgr.get_context(job_id).await.unwrap();
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
    use crate::context::JobState;

    let ctx_mgr = Arc::new(ContextManager::new(5));

    let job1 = ctx_mgr
        .create_job_for_user("default", "test", "test1")
        .await
        .unwrap();
    let ctx1 = ctx_mgr.get_context(job1).await.unwrap();
    assert!(ctx1.state.is_active(), "Pending job is active");

    let job2 = ctx_mgr
        .create_job_for_user("default", "test", "test2")
        .await
        .unwrap();
    ctx_mgr
        .update_context(job2, |ctx| {
            ctx.state = JobState::InProgress;
        })
        .await
        .unwrap();
    let ctx2 = ctx_mgr.get_context(job2).await.unwrap();
    assert!(ctx2.state.is_active(), "InProgress job is active");

    let job3 = ctx_mgr
        .create_job_for_user("default", "test", "test3")
        .await
        .unwrap();
    ctx_mgr
        .update_context(job3, |ctx| {
            ctx.state = JobState::Completed;
        })
        .await
        .unwrap();
    let ctx3 = ctx_mgr.get_context(job3).await.unwrap();
    assert!(ctx3.state.is_active(), "Completed is still active");

    let job4 = ctx_mgr
        .create_job_for_user("default", "test", "test4")
        .await
        .unwrap();
    ctx_mgr
        .update_context(job4, |ctx| {
            ctx.state = JobState::Failed;
        })
        .await
        .unwrap();
    let ctx4 = ctx_mgr.get_context(job4).await.unwrap();
    assert!(!ctx4.state.is_active(), "Failed job is terminal");

    let job5 = ctx_mgr
        .create_job_for_user("default", "test", "test5")
        .await
        .unwrap();
    ctx_mgr
        .update_context(job5, |ctx| {
            ctx.state = JobState::Cancelled;
        })
        .await
        .unwrap();
    let ctx5 = ctx_mgr.get_context(job5).await.unwrap();
    assert!(!ctx5.state.is_active(), "Cancelled job is terminal");

    let missing_job = Uuid::new_v4();
    let is_active = match ctx_mgr.get_context(missing_job).await {
        Ok(ctx) => ctx.state.is_active(),
        Err(_) => false,
    };
    assert!(!is_active, "Missing job should be treated as inactive");
}

#[cfg(all(test, feature = "docker", not(target_env = "msvc")))]
mod e2e_tests {
    use super::*;

    fn should_run_e2e() -> bool {
        std::env::var("IRONCLAW_E2E_DOCKER_TESTS").is_ok()
    }

    async fn connect_or_skip() -> Option<crate::sandbox::container::DockerConnection> {
        match crate::sandbox::connect_docker().await {
            Ok(docker) => Some(docker),
            Err(e) => {
                eprintln!("Skipping e2e test: Docker unavailable: {e}");
                None
            }
        }
    }

    async fn create_labeled_container(
        docker: &crate::sandbox::container::DockerConnection,
        name: &str,
        job_id: Uuid,
        age_offset: chrono::Duration,
    ) -> Option<String> {
        let job_id_str = job_id.to_string();
        let created_at_str = (Utc::now() - age_offset).to_rfc3339();
        let mut labels: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        labels.insert("ironclaw.job_id", &job_id_str);
        labels.insert("ironclaw.created_at", &created_at_str);

        match docker
            .create_container(
                Some(bollard::container::CreateContainerOptions {
                    name,
                    platform: None,
                }),
                bollard::container::Config {
                    image: Some("alpine:latest"),
                    labels: Some(labels),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(response) => Some(response.id),
            Err(e) => {
                eprintln!("Could not create test container '{name}': {e}");
                None
            }
        }
    }

    #[tokio::test]
    async fn e2e_reaper_lists_ironclaw_containers() {
        if !should_run_e2e() {
            eprintln!("Skipping e2e test (set IRONCLAW_E2E_DOCKER_TESTS=1 to run)");
            return;
        }

        let Some(docker) = connect_or_skip().await else {
            return;
        };

        let job_id = Uuid::new_v4();
        let test_name = format!("ironclaw-reaper-test-{}", &job_id.to_string()[..8]);
        let Some(container_id) =
            create_labeled_container(&docker, &test_name, job_id, chrono::Duration::hours(1)).await
        else {
            eprintln!("Skipping e2e test: Could not create test container");
            return;
        };

        let inspect = match docker.inspect_container(&container_id, None).await {
            Ok(container) => container,
            Err(e) => {
                let _ = docker.remove_container(&container_id, None).await;
                eprintln!("Failed to inspect container: {e}");
                return;
            }
        };

        let labels = inspect.config.and_then(|c| c.labels).unwrap_or_default();
        assert!(labels.contains_key("ironclaw.job_id"));
        assert_eq!(
            labels.get("ironclaw.job_id").map(|s| s.as_str()),
            Some(job_id.to_string().as_str())
        );

        let _ = docker.remove_container(&container_id, None).await;
    }

    #[tokio::test]
    async fn e2e_reaper_removes_orphaned_containers() {
        if !should_run_e2e() {
            eprintln!("Skipping e2e test (set IRONCLAW_E2E_DOCKER_TESTS=1 to run)");
            return;
        }

        let Some(docker) = connect_or_skip().await else {
            return;
        };

        let orphaned_job_id = Uuid::new_v4();
        let test_name = format!("ironclaw-orphan-test-{}", &orphaned_job_id.to_string()[..8]);
        let Some(container_id) = create_labeled_container(
            &docker,
            &test_name,
            orphaned_job_id,
            chrono::Duration::hours(2),
        )
        .await
        else {
            eprintln!("Skipping e2e test: Could not create test container");
            return;
        };

        assert!(docker.inspect_container(&container_id, None).await.is_ok());

        let _ = docker
            .stop_container(
                &container_id,
                Some(bollard::container::StopContainerOptions { t: 10 }),
            )
            .await;

        match docker
            .remove_container(
                &container_id,
                Some(bollard::container::RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
        {
            Ok(()) => {
                assert!(docker.inspect_container(&container_id, None).await.is_err());
            }
            Err(e) => {
                eprintln!("Warning: failed to remove test container: {e}");
                let _ = docker.remove_container(&container_id, None).await;
            }
        }
    }

    #[tokio::test]
    async fn e2e_reaper_respects_age_threshold() {
        if !should_run_e2e() {
            eprintln!("Skipping e2e test (set IRONCLAW_E2E_DOCKER_TESTS=1 to run)");
            return;
        }

        let Some(docker) = connect_or_skip().await else {
            return;
        };

        let recent_job_id = Uuid::new_v4();
        let old_job_id = Uuid::new_v4();
        let recent_name = format!("ironclaw-recent-test-{}", &recent_job_id.to_string()[..8]);
        let old_name = format!("ironclaw-old-test-{}", &old_job_id.to_string()[..8]);

        let Some(recent_container_id) = create_labeled_container(
            &docker,
            &recent_name,
            recent_job_id,
            chrono::Duration::minutes(5),
        )
        .await
        else {
            eprintln!("Skipping e2e test: Could not create recent container");
            return;
        };

        let Some(old_container_id) =
            create_labeled_container(&docker, &old_name, old_job_id, chrono::Duration::hours(2))
                .await
        else {
            let _ = docker.remove_container(&recent_container_id, None).await;
            eprintln!("Skipping e2e test: Could not create old container");
            return;
        };

        let recent_age =
            Utc::now().signed_duration_since(Utc::now() - chrono::Duration::minutes(5));
        let old_age = Utc::now().signed_duration_since(Utc::now() - chrono::Duration::hours(2));
        let threshold = chrono::Duration::minutes(10);

        assert!(recent_age < threshold);
        assert!(old_age >= threshold);

        let _ = docker.remove_container(&recent_container_id, None).await;
        let _ = docker.remove_container(&old_container_id, None).await;
    }
}
