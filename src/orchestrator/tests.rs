//! Tests for orchestrator setup using explicit startup inputs.

use super::container_job_config;
use crate::config::EnvContext;
use crate::testing::test_utils::test_config_from_context;

#[tokio::test]
async fn container_job_config_uses_overlay_only_anthropic_api_key() {
    let env_context = EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("DATABASE_URL", "unused://test")
        .with_secret("ANTHROPIC_API_KEY", "overlay-only-key");
    let config = test_config_from_context(&env_context)
        .await
        .expect("test configuration should resolve from the supplied context");

    let job_config = container_job_config(&config, &env_context, || None);

    assert_eq!(
        job_config.claude_code_api_key.as_deref(),
        Some("overlay-only-key")
    );
}

#[tokio::test]
async fn container_job_config_uses_credential_store_when_context_has_no_oauth_token() {
    let config_context = EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("DATABASE_URL", "unused://test");
    let config = test_config_from_context(&config_context)
        .await
        .expect("test configuration should resolve from the supplied context");
    let empty_context = EnvContext::default();

    let job_config = container_job_config(&config, &empty_context, || {
        Some("credential-store-token".to_string())
    });

    assert_eq!(
        job_config.claude_code_oauth_token.as_deref(),
        Some("credential-store-token")
    );
}

#[tokio::test]
async fn container_job_config_prefers_oauth_token_from_context() {
    let config_context = EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("DATABASE_URL", "unused://test");
    let config = test_config_from_context(&config_context)
        .await
        .expect("test configuration should resolve from the supplied context");
    let env_context = EnvContext::default().with_secret("ANTHROPIC_OAUTH_TOKEN", "context-token");

    let job_config = container_job_config(&config, &env_context, || {
        Some("credential-store-token".to_string())
    });

    assert_eq!(
        job_config.claude_code_oauth_token.as_deref(),
        Some("context-token")
    );
}
