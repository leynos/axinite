//! Docker end-to-end coverage for [`SandboxReaper`].

use super::*;
use crate::orchestrator::auth::TokenStore;
use crate::orchestrator::job_manager::{ContainerJobConfig, ContainerJobManager};

const LABEL_JOB_ID: &str = "ironclaw.job_id";
const LABEL_CREATED_AT: &str = "ironclaw.created_at";

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
    labels.insert(LABEL_JOB_ID, &job_id_str);
    labels.insert(LABEL_CREATED_AT, &created_at_str);

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

    let reaper = match SandboxReaper::new(
        Arc::new(ContainerJobManager::new(
            ContainerJobConfig::default(),
            TokenStore::new(),
        )),
        Arc::new(ContextManager::new(5)),
        ReaperConfig::default(),
    )
    .await
    {
        Ok(reaper) => reaper,
        Err(e) => {
            let _ = docker.remove_container(&container_id, None).await;
            eprintln!("Skipping e2e test: Could not create reaper: {e}");
            return;
        }
    };

    let containers = reaper
        .list_ironclaw_containers()
        .await
        .expect("list_ironclaw_containers failed in e2e_reaper_lists_ironclaw_containers");
    assert!(
        containers
            .iter()
            .any(|(id, listed_job_id, _)| id == &container_id && listed_job_id == &job_id),
        "expected reaper to discover the labelled test container"
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

    let reaper = match SandboxReaper::new(
        Arc::new(ContainerJobManager::new(
            ContainerJobConfig::default(),
            TokenStore::new(),
        )),
        Arc::new(ContextManager::new(5)),
        ReaperConfig::default(),
    )
    .await
    {
        Ok(reaper) => reaper,
        Err(e) => {
            let _ = docker.remove_container(&container_id, None).await;
            eprintln!("Skipping e2e test: Could not create reaper: {e}");
            return;
        }
    };

    reaper.scan_and_reap().await;
    assert!(docker.inspect_container(&container_id, None).await.is_err());
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
        create_labeled_container(&docker, &old_name, old_job_id, chrono::Duration::hours(2)).await
    else {
        let _ = docker.remove_container(&recent_container_id, None).await;
        eprintln!("Skipping e2e test: Could not create old container");
        return;
    };

    let reaper = match SandboxReaper::new(
        Arc::new(ContainerJobManager::new(
            ContainerJobConfig::default(),
            TokenStore::new(),
        )),
        Arc::new(ContextManager::new(5)),
        ReaperConfig::default(),
    )
    .await
    {
        Ok(reaper) => reaper,
        Err(e) => {
            let _ = docker.remove_container(&recent_container_id, None).await;
            let _ = docker.remove_container(&old_container_id, None).await;
            eprintln!("Skipping e2e test: Could not create reaper: {e}");
            return;
        }
    };

    reaper.scan_and_reap().await;
    assert!(
        docker
            .inspect_container(&recent_container_id, None)
            .await
            .is_ok()
    );
    assert!(
        docker
            .inspect_container(&old_container_id, None)
            .await
            .is_err()
    );

    let _ = docker.remove_container(&recent_container_id, None).await;
    let _ = docker.remove_container(&old_container_id, None).await;
}
