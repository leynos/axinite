//! Bootstrap helpers for tool registration.

use std::sync::Arc;

use uuid::Uuid;

use crate::channels::IncomingMessage;
use crate::channels::web::types::SseEvent;
use crate::context::ContextManager;
use crate::db::Database;
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::secrets::SecretsStore;
use crate::tools::builtin::{PromptQueue, SchedulerSlot};
use crate::tools::{RegisterJobToolsConfig, ToolRegistry};

/// Dependency bundle for registering job tools during bootstrap.
pub struct JobToolsArgs {
    /// Shared conversation context store used by all job tools.
    pub context_manager: Arc<ContextManager>,
    /// Scheduler slot populated after the scheduler is initialized.
    pub scheduler_slot: Option<SchedulerSlot>,
    /// Optional sandbox-backed job manager.
    pub job_manager: Option<Arc<ContainerJobManager>>,
    /// Optional database handle for persistence-backed job tools.
    pub store: Option<Arc<dyn Database>>,
    /// Optional broadcast sender for job event fan-out.
    pub job_event_tx: Option<tokio::sync::broadcast::Sender<(Uuid, SseEvent)>>,
    /// Optional inbound injector for monitored job execution.
    pub inject_tx: Option<tokio::sync::mpsc::Sender<IncomingMessage>>,
    /// Optional prompt queue for interactive job prompts.
    pub prompt_queue: Option<PromptQueue>,
    /// Optional secrets store shared with job execution flows.
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

/// Register the job-management tool set from bootstrap wiring.
pub fn register_job_tools(registry: &ToolRegistry, args: JobToolsArgs) {
    registry.register_job_tools(RegisterJobToolsConfig {
        context_manager: args.context_manager,
        scheduler_slot: args.scheduler_slot,
        job_manager: args.job_manager,
        store: args.store,
        job_event_tx: args.job_event_tx,
        inject_tx: args.inject_tx,
        prompt_queue: args.prompt_queue,
        secrets_store: args.secrets_store,
    });
}
