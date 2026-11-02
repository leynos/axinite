//! Orchestrator for managing sandboxed worker containers.
//!
//! The orchestrator runs in the main agent process and provides:
//! - An internal HTTP API for worker communication (LLM proxy, status, secrets)
//! - Per-job bearer token authentication
//! - Container lifecycle management (create, monitor, stop)
//!
//! ```text
//! ┌───────────────────────────────────────────────┐
//! │              Orchestrator                       │
//! │                                                 │
//! │  Internal API (default :50051, configurable)    │
//! │    POST /worker/{job_id}/llm/complete           │
//! │    POST /worker/{job_id}/llm/complete_with_tools│
//! │    GET  /worker/{job_id}/job                    │
//! │    GET  /worker/{job_id}/tools/catalog          │
//! │    POST /worker/{job_id}/tools/execute          │
//! │    GET  /worker/{job_id}/credentials            │
//! │    POST /worker/{job_id}/status                 │
//! │    POST /worker/{job_id}/complete               │
//! │                                                 │
//! │  ContainerJobManager                            │
//! │    create_job() -> container + token             │
//! │    stop_job()                                    │
//! │    list_jobs()                                   │
//! │                                                 │
//! │  TokenStore                                     │
//! │    per-job bearer tokens (in-memory only)       │
//! │    per-job credential grants (in-memory only)   │
//! └───────────────────────────────────────────────┘
//! ```

pub mod api;
pub mod auth;
mod bind_mount;
pub mod job_manager;
mod job_registry;
pub mod job_types;
pub mod reaper;

pub use api::OrchestratorApi;
pub use auth::{CredentialGrant, TokenStore};
pub use job_manager::{
    CompletionResult, ContainerHandle, ContainerJobConfig, ContainerJobManager, JobMode,
};
pub use reaper::{ReaperConfig, SandboxReaper};

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::channels::web::types::SseEvent;
use crate::config::EnvContext;
use crate::db::Database;
use crate::llm::LlmProvider;
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;

/// Result of orchestrator setup, containing all handles needed by the agent.
pub struct OrchestratorSetup {
    /// Container job manager; `None` unless sandboxing is enabled and Docker is available.
    pub container_job_manager: Option<Arc<ContainerJobManager>>,
    /// Job event broadcast sender (capacity 256), present exactly when the job manager is.
    pub job_event_tx: Option<broadcast::Sender<(Uuid, SseEvent)>>,
    /// Pending follow-up prompts keyed by job ID, polled by workers between turns.
    pub prompt_queue: Arc<Mutex<HashMap<Uuid, VecDeque<api::PendingPrompt>>>>,
    /// Detected Docker status; `Disabled` when sandboxing is turned off.
    pub docker_status: crate::sandbox::DockerStatus,
}

struct OrchestratorRuntime {
    container_job_manager: Option<Arc<ContainerJobManager>>,
    job_event_tx: Option<broadcast::Sender<(Uuid, SseEvent)>>,
}

/// Startup inputs required to set up the container orchestrator.
pub struct OrchestratorSetupContext<'a> {
    /// Resolved application configuration.
    pub config: &'a crate::config::Config,
    /// Environment snapshot resolved during application startup.
    pub env_context: &'a EnvContext,
    /// Configured language model provider.
    pub llm: &'a Arc<dyn LlmProvider>,
    /// Available tools for orchestrator jobs.
    pub tools: &'a Arc<ToolRegistry>,
    /// Optional database used by the orchestrator API.
    pub db: Option<&'a Arc<dyn Database>>,
    /// Optional store used to retrieve channel secrets.
    pub secrets_store: Option<&'a Arc<dyn SecretsStore + Send + Sync>>,
}

/// Detect Docker availability, create the container job manager, and start
/// the orchestrator internal API in the background.
pub async fn setup_orchestrator(
    config: &crate::config::Config,
    llm: &Arc<dyn LlmProvider>,
    tools: &Arc<ToolRegistry>,
    db: Option<&Arc<dyn Database>>,
    secrets_store: Option<&Arc<dyn SecretsStore + Send + Sync>>,
) -> OrchestratorSetup {
    let env_context = EnvContext::capture_ambient();
    setup_orchestrator_from_context(OrchestratorSetupContext {
        config,
        env_context: &env_context,
        llm,
        tools,
        db,
        secrets_store,
    })
    .await
}

/// Set up the orchestrator from the environment snapshot resolved at startup.
pub async fn setup_orchestrator_from_context(
    context: OrchestratorSetupContext<'_>,
) -> OrchestratorSetup {
    let prompt_queue = Arc::new(Mutex::new(
        HashMap::<Uuid, VecDeque<api::PendingPrompt>>::new(),
    ));

    let docker_status = detect_docker_status(context.config.sandbox.enabled).await;
    let runtime = start_orchestrator_if_enabled(&context, docker_status, Arc::clone(&prompt_queue));

    OrchestratorSetup {
        container_job_manager: runtime.container_job_manager,
        job_event_tx: runtime.job_event_tx,
        prompt_queue,
        docker_status,
    }
}

/// Probe Docker when the sandbox is enabled and log why it is unavailable.
async fn detect_docker_status(sandbox_enabled: bool) -> crate::sandbox::DockerStatus {
    if sandbox_enabled {
        let detection = crate::sandbox::check_docker().await;
        match detection.status {
            crate::sandbox::DockerStatus::Available => {
                tracing::info!("Docker is available");
            }
            crate::sandbox::DockerStatus::NotInstalled => {
                tracing::warn!(
                    "Docker is not installed -- sandbox disabled for this session. {}",
                    detection.platform.install_hint()
                );
            }
            crate::sandbox::DockerStatus::NotRunning => {
                tracing::warn!(
                    "Docker is installed but not running -- sandbox disabled for this session. {}",
                    detection.platform.start_hint()
                );
            }
            crate::sandbox::DockerStatus::Disabled => {}
        }
        detection.status
    } else {
        crate::sandbox::DockerStatus::Disabled
    }
}

fn start_orchestrator_if_enabled(
    context: &OrchestratorSetupContext<'_>,
    docker_status: crate::sandbox::DockerStatus,
    prompt_queue: Arc<Mutex<HashMap<Uuid, VecDeque<api::PendingPrompt>>>>,
) -> OrchestratorRuntime {
    if context.config.sandbox.enabled && docker_status.is_ok() {
        let (tx, _) = broadcast::channel(256);
        let job_event_tx = Some(tx);

        let token_store = TokenStore::new();
        let job_config = container_job_config(
            context.config,
            context.env_context,
            crate::config::ClaudeCodeConfig::extract_oauth_token,
        );
        let jm = Arc::new(ContainerJobManager::new(job_config, token_store.clone()));

        let orchestrator_state = api::OrchestratorState {
            llm: Arc::clone(context.llm),
            tools: Arc::clone(context.tools),
            job_manager: Arc::clone(&jm),
            token_store,
            job_event_tx: job_event_tx.clone(),
            prompt_queue,
            store: context.db.cloned(),
            secrets_store: context.secrets_store.cloned(),
            user_id: "default".to_string(),
        };

        tokio::spawn(async move {
            if let Err(e) = OrchestratorApi::start(orchestrator_state, 50051).await {
                tracing::error!("Orchestrator API failed: {}", e);
            }
        });

        if context.config.claude_code.enabled {
            tracing::info!(
                "Claude Code sandbox mode available (model: {}, max_turns: {})",
                context.config.claude_code.model,
                context.config.claude_code.max_turns
            );
        }
        OrchestratorRuntime {
            job_event_tx,
            container_job_manager: Some(jm),
        }
    } else {
        OrchestratorRuntime {
            job_event_tx: None,
            container_job_manager: None,
        }
    }
}

fn container_job_config(
    config: &crate::config::Config,
    env_context: &EnvContext,
    credential_store_oauth_token: impl FnOnce() -> Option<String>,
) -> ContainerJobConfig {
    ContainerJobConfig {
        image: config.sandbox.image.clone(),
        memory_limit_mb: config.sandbox.memory_limit_mb,
        cpu_shares: config.sandbox.cpu_shares,
        orchestrator_port: 50051,
        claude_code_api_key: env_context.get_owned("ANTHROPIC_API_KEY"),
        claude_code_oauth_token: env_context
            .get_owned("ANTHROPIC_OAUTH_TOKEN")
            .or_else(credential_store_oauth_token),
        claude_code_model: config.claude_code.model.clone(),
        claude_code_max_turns: config.claude_code.max_turns,
        claude_code_memory_limit_mb: config.claude_code.memory_limit_mb,
        claude_code_allowed_tools: config.claude_code.allowed_tools.clone(),
    }
}

#[cfg(test)]
mod tests;
