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

/// Dependency bundle for job-tool registration.
pub struct RegisterJobToolsConfig {
    pub context_manager: Arc<ContextManager>,
    pub scheduler_slot: Option<crate::tools::builtin::SchedulerSlot>,
    pub job_manager: Option<Arc<ContainerJobManager>>,
    pub store: Option<Arc<dyn Database>>,
    pub job_event_tx:
        Option<tokio::sync::broadcast::Sender<(uuid::Uuid, crate::channels::web::types::SseEvent)>>,
    pub inject_tx: Option<tokio::sync::mpsc::Sender<crate::channels::IncomingMessage>>,
    pub prompt_queue: Option<PromptQueue>,
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
