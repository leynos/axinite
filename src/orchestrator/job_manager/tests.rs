use super::*;
use std::path::PathBuf;

#[cfg(not(feature = "docker"))]
use crate::sandbox::container::DOCKER_FEATURE_DISABLED_REASON;

fn sample_handle(job_id: Uuid) -> ContainerHandle {
    ContainerHandle {
        job_id,
        container_id: "container-123".to_string(),
        state: ContainerState::Running,
        mode: JobMode::Worker,
        created_at: chrono::Utc::now(),
        project_dir: None,
        task_description: "test job".to_string(),
        last_worker_status: None,
        worker_iteration: 0,
        completion_result: None,
    }
}

#[test]
fn test_container_job_config_default() {
    let config = ContainerJobConfig::default();
    assert_eq!(config.orchestrator_port, 50051);
    assert_eq!(config.memory_limit_mb, 2048);
}

#[test]
fn test_container_state_display() {
    assert_eq!(ContainerState::Running.to_string(), "running");
    assert_eq!(ContainerState::Stopped.to_string(), "stopped");
}

#[test]
fn test_validate_bind_mount_valid_path() {
    let base = crate::bootstrap::compute_ironclaw_base_dir().join("projects");
    std::fs::create_dir_all(&base).unwrap();

    let test_dir = base.join("test_validate_bind");
    std::fs::create_dir_all(&test_dir).unwrap();

    let result = validate_bind_mount_path(&test_dir, Uuid::new_v4());
    assert!(result.is_ok());
    let canonical = result.unwrap();
    assert!(canonical.starts_with(base.canonicalize().unwrap()));

    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
fn test_validate_bind_mount_rejects_outside_base() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().to_path_buf();

    let result = validate_bind_mount_path(&outside, Uuid::new_v4());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("outside allowed base"),
        "expected 'outside allowed base', got: {}",
        err
    );
}

#[test]
fn test_validate_bind_mount_rejects_nonexistent() {
    let nonexistent = PathBuf::from("/no/such/path/at/all");
    let result = validate_bind_mount_path(&nonexistent, Uuid::new_v4());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("canonicalize"),
        "expected canonicalize error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_update_worker_status() {
    let store = TokenStore::new();
    let mgr = ContainerJobManager::new(ContainerJobConfig::default(), store);
    let job_id = Uuid::new_v4();

    {
        let mut containers = mgr.containers.write().await;
        containers.insert(job_id, sample_handle(job_id));
    }

    mgr.update_worker_status(job_id, Some("Iteration 3".to_string()), 3)
        .await;

    let handle = mgr.get_handle(job_id).await.unwrap();
    assert_eq!(handle.worker_iteration, 3);
    assert_eq!(handle.last_worker_status.as_deref(), Some("Iteration 3"));
}

#[cfg(not(feature = "docker"))]
#[tokio::test]
async fn create_job_fails_no_docker() {
    let token_store = TokenStore::new();
    let manager = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    let job_id = Uuid::new_v4();
    let grants = vec![CredentialGrant {
        secret_name: "github_token".to_string(),
        env_var: "GITHUB_TOKEN".to_string(),
    }];

    let error = manager
        .create_job(job_id, "test task", None, JobMode::Worker, grants)
        .await
        .unwrap_err();

    match error {
        OrchestratorError::Docker { reason } => {
            assert!(
                reason.contains(DOCKER_FEATURE_DISABLED_REASON),
                "expected disabled Docker error, got: {reason}"
            );
        }
        other => panic!("expected Docker error, got {other:?}"),
    }

    assert!(manager.get_handle(job_id).await.is_none());
    assert_eq!(token_store.active_count().await, 0);
    assert!(token_store.get_grants(job_id).await.is_none());
}

#[cfg(not(feature = "docker"))]
#[tokio::test]
async fn complete_job_no_docker_retains_result_and_revokes_token() {
    let token_store = TokenStore::new();
    let manager = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    let job_id = Uuid::new_v4();
    let token = token_store.create_token(job_id).await;
    token_store
        .store_grants(
            job_id,
            vec![CredentialGrant {
                secret_name: "github_token".to_string(),
                env_var: "GITHUB_TOKEN".to_string(),
            }],
        )
        .await;

    {
        let mut containers = manager.containers.write().await;
        containers.insert(job_id, sample_handle(job_id));
    }

    let result = CompletionResult {
        success: true,
        message: Some("done".to_string()),
    };

    manager.complete_job(job_id, result.clone()).await.unwrap();

    let handle = manager.get_handle(job_id).await.unwrap();
    assert_eq!(handle.state, ContainerState::Stopped);
    assert_eq!(handle.completion_result, Some(result));
    assert!(!token_store.validate(job_id, &token).await);
    assert!(token_store.get_grants(job_id).await.is_none());
}
