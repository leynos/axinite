//! Tests for project directory resolution and job ID prefix resolution.

use uuid::Uuid;

use crate::context::ContextManager;
use crate::tools::builtin::job::project_dir::{projects_base, resolve_project_dir};
use crate::tools::builtin::job::resolve_job_id;

#[test]
fn test_resolve_project_dir_auto() {
    let project_id = Uuid::new_v4();
    let (dir, browse_id) = resolve_project_dir(None, project_id).unwrap();
    assert!(dir.exists());
    assert!(dir.ends_with(project_id.to_string()));
    assert_eq!(browse_id, project_id.to_string());

    // Must be under the projects base
    let base = projects_base().canonicalize().unwrap();
    assert!(dir.starts_with(&base));

    let _ = ambient_fs::remove_dir_all(&dir);
}

#[test]
fn test_resolve_project_dir_explicit_under_base() {
    let base = projects_base();
    ambient_fs::create_dir_all(&base).unwrap();
    let explicit = base.join("test_explicit_project");
    // Explicit paths must already exist (no auto-create).
    ambient_fs::create_dir_all(&explicit).unwrap();
    let project_id = Uuid::new_v4();

    let (dir, browse_id) = resolve_project_dir(Some(explicit.clone()), project_id).unwrap();
    assert!(dir.exists());
    assert_eq!(browse_id, "test_explicit_project");

    let canonical_base = base.canonicalize().unwrap();
    assert!(dir.starts_with(&canonical_base));

    let _ = ambient_fs::remove_dir_all(&explicit);
}

#[test]
fn test_resolve_project_dir_rejects_outside_base() {
    let tmp = tempfile::tempdir().unwrap();
    let escape_attempt = tmp.path().join("evil_project");
    // Don't create it: explicit paths that don't exist are rejected
    // before the prefix check even runs.

    let result = resolve_project_dir(Some(escape_attempt), Uuid::new_v4());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("does not exist"),
        "expected 'does not exist' error, got: {}",
        err
    );
}

#[test]
fn test_resolve_project_dir_rejects_outside_base_existing() {
    // A directory that exists but is outside the projects base.
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().to_path_buf();

    let result = resolve_project_dir(Some(outside), Uuid::new_v4());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("must be under"),
        "expected 'must be under' error, got: {}",
        err
    );
}

#[test]
fn test_resolve_project_dir_rejects_traversal() {
    // Non-existent traversal path is rejected because canonicalize fails.
    let base = projects_base();
    let traversal = base.join("legit").join("..").join("..").join(".ssh");

    let result = resolve_project_dir(Some(traversal), Uuid::new_v4());
    assert!(result.is_err(), "traversal path should be rejected");

    // Traversal path that actually resolves gets the prefix check.
    // `base/../` resolves to the parent of projects base, which is outside.
    let base_parent = projects_base().join("..").join("definitely_not_projects");
    ambient_fs::create_dir_all(&base_parent).ok();
    if base_parent.exists() {
        let result = resolve_project_dir(Some(base_parent.clone()), Uuid::new_v4());
        assert!(result.is_err(), "path outside base should be rejected");
        let _ = ambient_fs::remove_dir_all(&base_parent);
    }
}

#[tokio::test]
async fn test_resolve_job_id_full_uuid() {
    let cm = ContextManager::new(5);
    let job_id = cm.create_job("Test", "Desc").await.unwrap();

    let resolved = resolve_job_id(&job_id.to_string(), &cm).await.unwrap();
    assert_eq!(resolved, job_id);
}

#[tokio::test]
async fn test_resolve_job_id_short_prefix() {
    let cm = ContextManager::new(5);
    let job_id = cm.create_job("Test", "Desc").await.unwrap();

    // Use first 8 hex chars (without dashes)
    let hex = job_id.to_string().replace('-', "");
    let prefix = &hex[..8];
    let resolved = resolve_job_id(prefix, &cm).await.unwrap();
    assert_eq!(resolved, job_id);
}

#[tokio::test]
async fn test_resolve_job_id_no_match() {
    let cm = ContextManager::new(5);
    cm.create_job("Test", "Desc").await.unwrap();

    let result = resolve_job_id("00000000", &cm).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("no job found"),
        "expected 'no job found', got: {}",
        err
    );
}

#[tokio::test]
async fn test_resolve_job_id_invalid_input() {
    let cm = ContextManager::new(5);
    let result = resolve_job_id("not-hex-at-all!", &cm).await;
    assert!(result.is_err());
}
