use std::path::PathBuf;
use std::sync::Arc;

use axum::{Router, routing::get};
use rstest::fixture;

use super::super::*;
use crate::channels::web::handlers::oauth::{
    oauth_callback_handler, slack_relay_oauth_callback_handler,
};
use crate::testing::credentials::TEST_GATEWAY_CRYPTO_KEY;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TestGatewayStateFactory;

impl TestGatewayStateFactory {
    pub(super) fn build(
        self,
        ext_mgr: Option<Arc<ExtensionManager>>,
        workspace: Option<Arc<Workspace>>,
    ) -> Arc<GatewayState> {
        Arc::new(GatewayState {
            msg_tx: tokio::sync::RwLock::new(None),
            sse: SseManager::new(),
            workspace,
            session_manager: None,
            log_broadcaster: None,
            log_level_handle: None,
            extension_manager: ext_mgr,
            tool_registry: None,
            store: None,
            job_manager: None,
            prompt_queue: None,
            user_id: "test".to_string(),
            shutdown_tx: tokio::sync::RwLock::new(None),
            ws_tracker: None,
            llm_provider: None,
            skill_registry: None,
            skill_catalog: None,
            scheduler: None,
            chat_rate_limiter: RateLimiter::new(30, 60),
            oauth_rate_limiter: RateLimiter::new(10, 60),
            registry_entries: vec![],
            cost_guard: None,
            routine_engine: Arc::new(tokio::sync::RwLock::new(None)),
            startup_time: std::time::Instant::now(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TestOAuthRouterFactory;

impl TestOAuthRouterFactory {
    pub(super) fn build(self, state: Arc<GatewayState>) -> Router {
        Router::new()
            .route("/oauth/callback", get(oauth_callback_handler))
            .with_state(state)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TestRelayOAuthRouterFactory;

impl TestRelayOAuthRouterFactory {
    pub(super) fn build(self, state: Arc<GatewayState>) -> Router {
        Router::new()
            .route(
                "/oauth/slack/callback",
                get(slack_relay_oauth_callback_handler),
            )
            .with_state(state)
    }
}

#[fixture]
pub(super) fn test_gateway_state() -> TestGatewayStateFactory {
    TestGatewayStateFactory
}

#[fixture]
pub(super) fn test_oauth_router() -> TestOAuthRouterFactory {
    TestOAuthRouterFactory
}

#[fixture]
pub(super) fn test_relay_oauth_router() -> TestRelayOAuthRouterFactory {
    TestRelayOAuthRouterFactory
}

pub(super) fn build_test_secrets_store() -> Arc<dyn crate::secrets::SecretsStore + Send + Sync> {
    Arc::new(crate::secrets::InMemorySecretsStore::new(Arc::new(
        crate::secrets::SecretsCrypto::new(secrecy::SecretString::from(
            TEST_GATEWAY_CRYPTO_KEY.to_string(),
        ))
        .expect("crypto"),
    )))
}

pub(super) fn build_test_ext_mgr(
    secrets: Arc<dyn crate::secrets::SecretsStore + Send + Sync>,
) -> Arc<ExtensionManager> {
    let tool_registry = Arc::new(ToolRegistry::new());
    let mcp_sm = Arc::new(crate::tools::mcp::session::McpSessionManager::new());
    let mcp_pm = Arc::new(crate::tools::mcp::process::McpProcessManager::new());
    Arc::new(ExtensionManager::new(
        mcp_sm,
        mcp_pm,
        secrets,
        tool_registry,
        None,
        None,
        PathBuf::from("/tmp/wasm_tools"),
        PathBuf::from("/tmp/wasm_channels"),
        None,
        "test".to_string(),
        None,
        vec![],
    ))
}
