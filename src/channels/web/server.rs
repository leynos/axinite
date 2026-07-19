//! Axum HTTP server bootstrap for the web gateway.
//!
//! This module owns shared gateway state, rate limiting, and top-level router
//! composition. Domain-specific handlers live under `handlers/`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::header,
    middleware,
    routing::{get, post},
};
use tokio::sync::{mpsc, oneshot};
use tower_http::cors::{AllowHeaders, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::agent::SessionManager;
use crate::channels::IncomingMessage;
use crate::channels::web::auth::{AuthState, auth_middleware};
use crate::channels::web::handlers::{
    chat, extensions, features, jobs, memory, oauth, pairing, routines, settings, skills,
    static_files, ui_assets,
};
use crate::channels::web::log_layer::LogBroadcaster;
use crate::channels::web::sse::SseManager;
use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

pub use crate::channels::web::handlers::chat_auth::clear_auth_mode;
pub use crate::channels::web::handlers::oauth::oauth_callback_handler;
pub use crate::channels::web::handlers::oauth_slack::slack_relay_oauth_callback_handler;

/// Shared prompt queue: maps job IDs to pending follow-up prompts for Claude Code bridges.
pub type PromptQueue = Arc<
    tokio::sync::Mutex<
        std::collections::HashMap<
            uuid::Uuid,
            std::collections::VecDeque<crate::orchestrator::api::PendingPrompt>,
        >,
    >,
>;

/// Slot for the routine engine, filled at runtime after the agent starts.
pub type RoutineEngineSlot =
    Arc<tokio::sync::RwLock<Option<Arc<crate::agent::routine_engine::RoutineEngine>>>>;

/// Simple sliding-window rate limiter.
///
/// Tracks the number of requests in the current window. Resets when the window expires.
/// Not per-IP (since this is a single-user gateway with auth), but prevents flooding.
pub struct RateLimiter {
    /// Requests remaining in the current window.
    remaining: AtomicU64,
    /// Epoch second when the current window started.
    window_start: AtomicU64,
    /// Maximum requests per window.
    max_requests: u64,
    /// Window duration in seconds.
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_requests: u64, window_secs: u64) -> Self {
        Self {
            remaining: AtomicU64::new(max_requests),
            window_start: AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
            max_requests,
            window_secs,
        }
    }

    /// Try to consume one request. Returns `true` if allowed, `false` if rate limited.
    pub fn check(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let window = self.window_start.load(Ordering::Relaxed);
        if now.saturating_sub(window) >= self.window_secs {
            // Window expired, reset
            self.window_start.store(now, Ordering::Relaxed);
            self.remaining
                .store(self.max_requests - 1, Ordering::Relaxed);
            return true;
        }

        // Try to decrement remaining
        loop {
            let current = self.remaining.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self
                .remaining
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }
}

/// Shared state for all gateway handlers.
pub struct GatewayState {
    /// Channel to send messages to the agent loop.
    pub msg_tx: tokio::sync::RwLock<Option<mpsc::Sender<IncomingMessage>>>,
    /// SSE broadcast manager.
    pub sse: SseManager,
    /// Workspace for memory API.
    pub workspace: Option<Arc<Workspace>>,
    /// Session manager for thread info.
    pub session_manager: Option<Arc<SessionManager>>,
    /// Log broadcaster for the logs SSE endpoint.
    pub log_broadcaster: Option<Arc<LogBroadcaster>>,
    /// Handle for changing the tracing log level at runtime.
    pub log_level_handle: Option<Arc<crate::channels::web::log_layer::LogLevelHandle>>,
    /// Extension manager for extension management API.
    pub extension_manager: Option<Arc<ExtensionManager>>,
    /// Tool registry for listing registered tools.
    pub tool_registry: Option<Arc<ToolRegistry>>,
    /// Database store for sandbox job persistence.
    pub store: Option<Arc<dyn Database>>,
    /// Container job manager for sandbox operations.
    pub job_manager: Option<Arc<ContainerJobManager>>,
    /// Prompt queue for Claude Code follow-up prompts.
    pub prompt_queue: Option<PromptQueue>,
    /// User ID for this gateway.
    pub user_id: String,
    /// Shutdown signal sender.
    pub shutdown_tx: tokio::sync::RwLock<Option<oneshot::Sender<()>>>,
    /// WebSocket connection tracker.
    pub ws_tracker: Option<Arc<crate::channels::web::ws::WsConnectionTracker>>,
    /// LLM provider for OpenAI-compatible API proxy.
    pub llm_provider: Option<Arc<dyn crate::llm::LlmProvider>>,
    /// Skill registry for skill management API.
    pub skill_registry: Option<Arc<std::sync::RwLock<crate::skills::SkillRegistry>>>,
    /// Skill catalogue for searching the ClawHub registry.
    pub skill_catalog: Option<Arc<crate::skills::catalog::SkillCatalog>>,
    /// Scheduler for sending follow-up messages to running agent jobs.
    pub scheduler: Option<crate::tools::builtin::SchedulerSlot>,
    /// Rate limiter for chat endpoints (30 messages per 60 seconds).
    pub chat_rate_limiter: RateLimiter,
    /// Rate limiter for OAuth callback endpoints (10 requests per 60 seconds).
    pub oauth_rate_limiter: RateLimiter,
    /// Registry catalogue entries for the available extensions API.
    /// Populated at startup from `registry/` manifests, independent of extension manager.
    pub registry_entries: Vec<crate::extensions::RegistryEntry>,
    /// Cost guard for token/cost tracking.
    pub cost_guard: Option<Arc<crate::agent::cost_guard::CostGuard>>,
    /// Routine engine slot for manual routine triggering (filled at runtime).
    pub routine_engine: RoutineEngineSlot,
    /// Server startup time for uptime calculation.
    pub startup_time: std::time::Instant,
}

/// Bind the TCP listener and resolve the actual bound address.
async fn bind_listener(
    addr: SocketAddr,
) -> Result<(tokio::net::TcpListener, SocketAddr), crate::error::ChannelError> {
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        crate::error::ChannelError::StartupFailed {
            name: "gateway".to_string(),
            reason: format!("Failed to bind to {}: {}", addr, e),
        }
    })?;
    let bound_addr =
        listener
            .local_addr()
            .map_err(|e| crate::error::ChannelError::StartupFailed {
                name: "gateway".to_string(),
                reason: format!("Failed to get local addr: {}", e),
            })?;
    Ok((listener, bound_addr))
}

/// Compose the auth-protected route tree.
fn protected_routes(auth_token: String) -> Router<Arc<GatewayState>> {
    let auth_state = AuthState { token: auth_token };
    chat::routes()
        .merge(features::routes())
        .merge(memory::routes())
        .merge(jobs::routes())
        .merge(static_files::protected_routes())
        .merge(extensions::routes())
        .merge(pairing::routes())
        .merge(routines::routes())
        .merge(skills::routes())
        .merge(settings::routes())
        .route(
            "/v1/chat/completions",
            post(super::openai_compat::chat_completions_handler),
        )
        .route("/v1/models", get(super::openai_compat::models_handler))
        .route_layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
}

/// Build the CORS layer.
///
/// CORS: restrict to same-origin by default. Only localhost/127.0.0.1
/// origins are allowed, since the gateway is a local-first service.
fn build_cors_layer(addr: SocketAddr) -> Result<CorsLayer, crate::error::ChannelError> {
    let parse_origin = |origin: String| {
        origin.parse::<header::HeaderValue>().map_err(|e| {
            crate::error::ChannelError::StartupFailed {
                name: "gateway".to_string(),
                reason: format!("valid origin expected, got {origin:?}: {e}"),
            }
        })
    };
    Ok(CorsLayer::new()
        .allow_origin([
            parse_origin(format!("http://{}:{}", addr.ip(), addr.port()))?,
            parse_origin(format!("http://localhost:{}", addr.port()))?,
        ])
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
        ])
        .allow_headers(AllowHeaders::list([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
        ]))
        .allow_credentials(true))
}

/// Assemble the full application router with body limits, CORS, and
/// security headers.
fn build_app(state: Arc<GatewayState>, auth_token: String, cors: CorsLayer) -> Router {
    let public = oauth::public_routes().merge(ui_assets::public_routes());
    Router::new()
        .merge(public)
        .merge(protected_routes(auth_token))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024)) // 10 MB max request body (image uploads)
        .layer(cors)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            header::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            header::HeaderValue::from_static("DENY"),
        ))
        .with_state(state)
}

/// Start the gateway HTTP server.
///
/// Returns the actual bound `SocketAddr` (useful when binding to port 0).
pub async fn start_server(
    addr: SocketAddr,
    state: Arc<GatewayState>,
    auth_token: String,
) -> Result<SocketAddr, crate::error::ChannelError> {
    let (listener, bound_addr) = bind_listener(addr).await?;

    let cors = build_cors_layer(addr)?;
    let app = build_app(state.clone(), auth_token, cors);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    *state.shutdown_tx.write().await = Some(shutdown_tx);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
                tracing::debug!("Web gateway shutting down");
            })
            .await
        {
            tracing::error!("Web gateway server error: {}", e);
        }
    });

    Ok(bound_addr)
}

#[cfg(test)]
mod tests;
