//! Tests for `CreateJobTool`: local execution, schema shape, timeouts, and
//! credential grant parsing.

use std::sync::Arc;
use std::time::Duration;

use crate::context::{ContextManager, JobContext};
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::secrets::SecretsStore;
use crate::tools::builtin::job::CreateJobTool;
use crate::tools::tool::NativeTool;

#[tokio::test]
async fn test_create_job_tool_local() {
    let manager = Arc::new(ContextManager::new(5));
    let tool = CreateJobTool::new(manager.clone());

    // Without sandbox deps, it should use the local path
    assert!(!tool.sandbox_enabled());

    let params = serde_json::json!({
        "title": "Test Job",
        "description": "A test job description"
    });

    let ctx = JobContext::default();
    let result = tool.execute(params, &ctx).await.unwrap();

    let job_id = result.result.get("job_id").unwrap().as_str().unwrap();
    assert!(!job_id.is_empty());
    assert_eq!(
        result.result.get("status").unwrap().as_str().unwrap(),
        "pending"
    );
}

#[test]
fn test_schema_changes_with_sandbox() {
    let manager = Arc::new(ContextManager::new(5));

    // Without sandbox
    let tool = CreateJobTool::new(Arc::clone(&manager));
    let schema = tool.parameters_schema();
    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("title"));
    assert!(props.contains_key("description"));
    assert!(!props.contains_key("wait"));
    assert!(!props.contains_key("mode"));
}

#[test]
fn test_execution_timeout_sandbox() {
    let manager = Arc::new(ContextManager::new(5));

    // Without sandbox: default timeout
    let tool = CreateJobTool::new(Arc::clone(&manager));
    assert_eq!(tool.execution_timeout(), Duration::from_secs(30));
}

#[tokio::test]
async fn test_create_job_params() {
    let manager = Arc::new(ContextManager::new(5));
    let tool = CreateJobTool::new(manager);
    let ctx = JobContext::default();

    let missing_title = tool
        .execute(serde_json::json!({ "description": "A test job" }), &ctx)
        .await;
    assert!(missing_title.is_err());
    assert!(
        missing_title
            .unwrap_err()
            .to_string()
            .contains("missing 'title' parameter")
    );

    let missing_description = tool
        .execute(serde_json::json!({ "title": "Test Job" }), &ctx)
        .await;
    assert!(missing_description.is_err());
    assert!(
        missing_description
            .unwrap_err()
            .to_string()
            .contains("missing 'description' parameter")
    );
}

#[test]
fn test_sandbox_schema_includes_project_dir() {
    let manager = Arc::new(ContextManager::new(5));
    let jm = Arc::new(ContainerJobManager::new(
        crate::orchestrator::job_manager::ContainerJobConfig::default(),
        crate::orchestrator::TokenStore::new(),
    ));
    let tool = CreateJobTool::new(manager).with_sandbox(jm, None);
    let schema = tool.parameters_schema();
    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(
        props.contains_key("project_dir"),
        "sandbox schema must expose project_dir"
    );
}

#[test]
fn test_sandbox_schema_includes_credentials() {
    let manager = Arc::new(ContextManager::new(5));
    let jm = Arc::new(ContainerJobManager::new(
        crate::orchestrator::job_manager::ContainerJobConfig::default(),
        crate::orchestrator::TokenStore::new(),
    ));
    let tool = CreateJobTool::new(manager).with_sandbox(jm, None);
    let schema = tool.parameters_schema();
    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(
        props.contains_key("credentials"),
        "sandbox schema must expose credentials"
    );
}

#[tokio::test]
async fn test_parse_credentials_empty() {
    let manager = Arc::new(ContextManager::new(5));
    let tool = CreateJobTool::new(manager);

    // No credentials parameter
    let params = serde_json::json!({"title": "t", "description": "d"});
    let grants = tool.parse_credentials(&params, "user1").await.unwrap();
    assert!(grants.is_empty());

    // Empty credentials object
    let params = serde_json::json!({"credentials": {}});
    let grants = tool.parse_credentials(&params, "user1").await.unwrap();
    assert!(grants.is_empty());
}

#[tokio::test]
async fn test_parse_credentials_no_secrets_store() {
    let manager = Arc::new(ContextManager::new(5));
    let tool = CreateJobTool::new(manager);

    let params = serde_json::json!({"credentials": {"my_secret": "MY_SECRET"}});
    let result = tool.parse_credentials(&params, "user1").await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("no secrets store"),
        "expected 'no secrets store' error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_parse_credentials_missing_secret() {
    use crate::testing::credentials::test_secrets_store;

    let manager = Arc::new(ContextManager::new(5));
    let secrets: Arc<dyn SecretsStore + Send + Sync> = Arc::new(test_secrets_store());

    let tool = CreateJobTool::new(manager).with_secrets(Arc::clone(&secrets));

    let params = serde_json::json!({"credentials": {"nonexistent_secret": "SOME_VAR"}});
    let result = tool.parse_credentials(&params, "user1").await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "expected 'not found' error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_parse_credentials_valid() {
    use crate::secrets::CreateSecretParams;
    use crate::testing::credentials::{TEST_GITHUB_TOKEN, test_secrets_store};

    let manager = Arc::new(ContextManager::new(5));
    let secrets: Arc<dyn SecretsStore + Send + Sync> = Arc::new(test_secrets_store());

    // Store a secret
    secrets
        .create(
            "user1",
            CreateSecretParams::new("github_token", TEST_GITHUB_TOKEN),
        )
        .await
        .unwrap();

    let tool = CreateJobTool::new(manager).with_secrets(Arc::clone(&secrets));

    let params = serde_json::json!({
        "credentials": {"github_token": "GITHUB_TOKEN"}
    });
    let grants = tool.parse_credentials(&params, "user1").await.unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].secret_name, "github_token");
    assert_eq!(grants[0].env_var, "GITHUB_TOKEN");
}
