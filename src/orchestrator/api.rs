//! Internal HTTP API for worker-to-orchestrator communication.
//!
//! This runs on a separate port (default 50051) from the web gateway.
//! All endpoints are authenticated via per-job bearer tokens.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::channels::web::types::SseEvent;
use crate::db::Database;
use crate::llm::LlmProvider;
use crate::orchestrator::auth::{TokenStore, worker_auth_middleware};
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;

mod handler_support;
mod handlers;
mod remote_tools;

use crate::worker::api::{
    COMPLETE_ROUTE, CREDENTIALS_ROUTE, EVENT_ROUTE, JOB_ROUTE, LLM_COMPLETE_ROUTE,
    LLM_COMPLETE_WITH_TOOLS_ROUTE, PROMPT_ROUTE, REMOTE_TOOL_CATALOG_ROUTE,
    REMOTE_TOOL_EXECUTE_ROUTE, STATUS_ROUTE, WORKER_HEALTH_ROUTE,
};
use handler_support::{get_credentials_handler, get_prompt_handler};
use handlers::{
    execute_remote_tool, get_job, get_remote_tool_catalog, health_check, job_event_handler,
    llm_complete, llm_complete_with_tools, report_complete, report_status,
};

/// A follow-up prompt queued for a Claude Code bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPrompt {
    pub content: String,
    pub done: bool,
}

/// Shared state for the orchestrator API.
#[derive(Clone)]
pub struct OrchestratorState {
    pub llm: Arc<dyn LlmProvider>,
    pub tools: Arc<ToolRegistry>,
    pub job_manager: Arc<ContainerJobManager>,
    pub token_store: TokenStore,
    /// Broadcast channel for job events (consumed by the web gateway SSE).
    pub job_event_tx: Option<broadcast::Sender<(Uuid, SseEvent)>>,
    /// Buffered follow-up prompts for sandbox jobs, keyed by job_id.
    pub prompt_queue: Arc<Mutex<HashMap<Uuid, VecDeque<PendingPrompt>>>>,
    /// Database handle for persisting job events.
    pub store: Option<Arc<dyn Database>>,
    /// Encrypted secrets store for credential injection into containers.
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// User ID for secret lookups (single-tenant, typically "default").
    pub user_id: String,
}

/// The orchestrator's internal API server.
pub struct OrchestratorApi;

impl OrchestratorApi {
    /// Build the axum router for the internal API.
    pub fn router(state: OrchestratorState) -> Router {
        Router::new()
            // Worker routes: authenticated via route_layer middleware.
            .route(JOB_ROUTE, get(get_job))
            .route(LLM_COMPLETE_ROUTE, post(llm_complete))
            .route(LLM_COMPLETE_WITH_TOOLS_ROUTE, post(llm_complete_with_tools))
            .route(REMOTE_TOOL_CATALOG_ROUTE, get(get_remote_tool_catalog))
            .route(REMOTE_TOOL_EXECUTE_ROUTE, post(execute_remote_tool))
            .route(STATUS_ROUTE, post(report_status))
            .route(COMPLETE_ROUTE, post(report_complete))
            .route(EVENT_ROUTE, post(job_event_handler))
            .route(PROMPT_ROUTE, get(get_prompt_handler))
            .route(CREDENTIALS_ROUTE, get(get_credentials_handler))
            .route_layer(axum::middleware::from_fn_with_state(
                state.token_store.clone(),
                worker_auth_middleware,
            ))
            // Unauthenticated routes (added after the layer).
            .route(WORKER_HEALTH_ROUTE, get(health_check))
            .with_state(state)
    }

    /// Start the internal API server on the given port.
    ///
    /// On macOS/Windows (Docker Desktop), binds to loopback only because
    /// Docker Desktop routes `host.docker.internal` through its VM to the
    /// host's `127.0.0.1`.
    ///
    /// On Linux, containers reach the host via the docker bridge gateway
    /// (`172.17.0.1`), which is NOT loopback. Binding to `127.0.0.1`
    /// would reject container traffic. We bind to all interfaces instead
    /// and rely on `worker_auth_middleware` (applied as a route_layer on
    /// every `/worker/` endpoint) to reject unauthenticated requests.
    pub async fn start(
        state: OrchestratorState,
        port: u16,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let router = Self::router(state);
        let addr = if cfg!(target_os = "linux") {
            std::net::SocketAddr::from(([0, 0, 0, 0], port))
        } else {
            std::net::SocketAddr::from(([127, 0, 0, 1], port))
        };

        tracing::info!("Orchestrator internal API listening on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests;
