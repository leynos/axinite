//! Shared fixtures and router factories for web gateway route tests.

use std::sync::Arc;

use axum::{Router, routing::get};
use rstest::fixture;

use super::super::*;
use crate::channels::web::handlers::oauth::oauth_callback_handler;
use crate::channels::web::handlers::oauth_slack::slack_relay_oauth_callback_handler;
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
        .expect("construct test gateway secrets crypto"),
    )))
}

pub(super) fn build_test_ext_mgr(
    secrets: Arc<dyn crate::secrets::SecretsStore + Send + Sync>,
) -> Arc<ExtensionManager> {
    let tool_registry = Arc::new(ToolRegistry::new());
    let mcp_clients = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    Arc::new(ExtensionManager::new(
        crate::extensions::ExtensionManagerConfig {
            discovery: Arc::new(crate::extensions::NoOpDiscovery),
            relay_config: None,
            gateway_token: None,
            mcp_activation: Arc::new(crate::extensions::NoOpMcpActivation),
            wasm_tool_activation: Arc::new(crate::extensions::NoOpWasmToolActivation),
            wasm_channel_activation: Arc::new(crate::extensions::NoOpWasmChannelActivation),
            mcp_clients,
            secrets,
            tool_registry,
            hooks: None,
            wasm_tools_dir: std::env::temp_dir().join("ironclaw_test_wasm_tools"),
            wasm_channels_dir: std::env::temp_dir().join("ironclaw_test_wasm_channels"),
            tunnel_url: None,
            user_id: "test".to_string(),
            store: None,
            catalog_entries: vec![],
        },
    ))
}

pub(super) fn expired_pending_oauth_flow(
    secrets: Arc<dyn crate::secrets::SecretsStore + Send + Sync>,
) -> crate::cli::oauth_defaults::PendingOAuthFlow {
    crate::cli::oauth_defaults::PendingOAuthFlow {
        extension_name: "test_tool".to_string(),
        display_name: "Test Tool".to_string(),
        token_url: "https://example.com/token".to_string(),
        client_id: "client123".to_string(),
        client_secret: None,
        redirect_uri: "https://example.com/oauth/callback".to_string(),
        code_verifier: None,
        access_token_field: "access_token".to_string(),
        secret_name: "test_token".to_string(),
        provider: None,
        validation_endpoint: None,
        scopes: vec![],
        user_id: "test".to_string(),
        secrets,
        sse_sender: None,
        gateway_token: None,
        created_at: std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(600))
            .expect("system uptime is too low to run expired OAuth flow tests"),
    }
}
