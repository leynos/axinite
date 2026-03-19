//! Job-tool registration helpers for the tool registry.

use std::sync::Arc;

use crate::context::ContextManager;
use crate::db::Database;
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::secrets::SecretsStore;
use crate::tools::builtin::{
    CancelJobTool, CreateJobTool, JobEventsTool, JobPromptTool, JobStatusTool, ListJobsTool,
    PromptQueue,
};

use super::ToolRegistry;

/// Dependency bundle for registering the job-management tool set.
///
/// This config is the entrypoint for `ToolRegistry::register_job_tools`.
/// Every field is optional except `context_manager`, and `None` disables the
/// related tool capability rather than causing registration to fail.
pub struct RegisterJobToolsConfig {
    /// Shared conversation context store used by all job tools.
    ///
    /// The registry clones this `Arc` into each registered tool so they all
    /// resolve jobs against the same context manager instance.
    pub context_manager: Arc<ContextManager>,
    /// Optional scheduler slot populated after late scheduler initialization.
    ///
    /// When present, `create_job` submits orchestrator-managed jobs through the
    /// shared scheduler handle. `None` leaves scheduler-backed execution disabled.
    pub scheduler_slot: Option<crate::tools::builtin::SchedulerSlot>,
    /// Optional container job manager for sandbox-backed job execution.
    ///
    /// This `Arc` must point at the same manager instance that owns the worker
    /// lifecycle when sandbox delegation is enabled.
    pub job_manager: Option<Arc<ContainerJobManager>>,
    /// Optional database handle for persistence-backed job tools.
    ///
    /// `Some` enables database-dependent tools such as `job_events` and also
    /// supplies storage to sandbox job creation. `None` disables those paths.
    pub store: Option<Arc<dyn Database>>,
    /// Optional broadcast channel for streaming job events to observers.
    ///
    /// This sender only has an effect when paired with `inject_tx`; both must
    /// be `Some` before monitoring dependencies are wired into `create_job`.
    pub job_event_tx:
        Option<tokio::sync::broadcast::Sender<(uuid::Uuid, crate::channels::web::types::SseEvent)>>,
    /// Optional inbound message injector used for monitored job execution.
    ///
    /// This channel must be provided alongside `job_event_tx`; supplying one
    /// without the other leaves monitoring support disabled.
    pub inject_tx: Option<tokio::sync::mpsc::Sender<crate::channels::IncomingMessage>>,
    /// Optional prompt queue handle for interactive job follow-up prompts.
    ///
    /// `Some` registers the `job_prompt` tool. `None` keeps prompt injection
    /// support out of the registry.
    pub prompt_queue: Option<PromptQueue>,
    /// Optional secrets store shared with job-creation flows.
    ///
    /// The `Arc` must outlive any registered tools because `create_job` clones
    /// it when building execution contexts that need secret resolution.
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

impl ToolRegistry {
    /// Register job management tools.
    ///
    /// Job tools allow the LLM to create, list, check status, and cancel jobs.
    /// When sandbox deps are provided, `create_job` automatically delegates to
    /// Docker containers. Otherwise it dispatches via the Scheduler (which
    /// persists to DB and spawns a worker).
    pub fn register_job_tools(&self, config: RegisterJobToolsConfig) {
        let RegisterJobToolsConfig {
            context_manager,
            scheduler_slot,
            job_manager,
            store,
            job_event_tx,
            inject_tx,
            prompt_queue,
            secrets_store,
        } = config;

        let mut create_tool = CreateJobTool::new(Arc::clone(&context_manager));
        if let Some(slot) = scheduler_slot {
            create_tool = create_tool.with_scheduler_slot(slot);
        }
        if let Some(jm) = job_manager {
            create_tool = create_tool.with_sandbox(jm, store.clone());
        }
        if let (Some(etx), Some(itx)) = (job_event_tx, inject_tx) {
            create_tool = create_tool.with_monitor_deps(etx, itx);
        }
        if let Some(secrets) = secrets_store {
            create_tool = create_tool.with_secrets(secrets);
        }
        self.register_sync(Arc::new(create_tool));
        self.register_sync(Arc::new(ListJobsTool::new(Arc::clone(&context_manager))));
        self.register_sync(Arc::new(JobStatusTool::new(Arc::clone(&context_manager))));
        self.register_sync(Arc::new(CancelJobTool::new(Arc::clone(&context_manager))));

        let mut job_tool_count = 4;

        if let Some(store) = store {
            self.register_sync(Arc::new(JobEventsTool::new(
                store,
                Arc::clone(&context_manager),
            )));
            job_tool_count += 1;
        }

        if let Some(pq) = prompt_queue {
            self.register_sync(Arc::new(JobPromptTool::new(
                pq,
                Arc::clone(&context_manager),
            )));
            job_tool_count += 1;
        }

        tracing::debug!("Registered {} job management tools", job_tool_count);
    }
}
