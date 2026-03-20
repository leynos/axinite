use std::sync::Arc;

use crate::agent::routine_engine::RoutineEngine;
use crate::channels::IncomingMessage;
use crate::channels::web::types::SseEvent;
use crate::context::ContextManager;
use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::llm::{LlmProvider, ToolDefinition};
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::secrets::SecretsStore;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::builder::{BuildSoftwareTool, BuilderConfig, LlmSoftwareBuilder};
use crate::tools::builtin::{
    ApplyPatchTool, CancelJobTool, CreateJobTool, EchoTool, ExtensionInfoTool, HttpTool,
    JobEventsTool, JobPromptTool, JobStatusTool, JsonTool, ListDirTool, ListJobsTool,
    MemoryReadTool, MemorySearchTool, MemoryTreeTool, MemoryWriteTool, PromptQueue, ReadFileTool,
    ShellTool, SkillInstallTool, SkillListTool, SkillRemoveTool, SkillSearchTool, TimeTool,
    ToolActivateTool, ToolAuthTool, ToolInstallTool, ToolListTool, ToolRemoveTool, ToolSearchTool,
    ToolUpgradeTool, WriteFileTool,
};
use crate::tools::registry::loader::ToolRegistry;
use crate::tools::tool::{Tool, ToolDomain};
use crate::workspace::Workspace;

impl ToolRegistry {
    /// Register all built-in tools.
    pub fn register_builtin_tools(&self) {
        self.register_sync(Arc::new(EchoTool));
        self.register_sync(Arc::new(TimeTool));
        self.register_sync(Arc::new(JsonTool));

        let mut http = HttpTool::new();
        if let (Some(cr), Some(ss)) = (&self.credential_registry, &self.secrets_store) {
            http = http.with_credentials(Arc::clone(cr), Arc::clone(ss));
        }
        self.register_sync(Arc::new(http));

        tracing::debug!("Registered {} built-in tools", self.count());
    }

    /// Register only orchestrator-domain tools (safe for the main process).
    ///
    /// This registers tools that don't touch the filesystem or run shell commands:
    /// echo, time, json, http. Use this when `allow_local_tools = false` and
    /// container-domain tools should only be available inside sandboxed containers.
    pub fn register_orchestrator_tools(&self) {
        self.register_builtin_tools();
        // register_builtin_tools already only registers orchestrator-domain tools
    }

    /// Register container-domain tools (filesystem, shell, code).
    ///
    /// These tools are intended to run inside sandboxed Docker containers.
    /// Call this in the worker process, not the orchestrator (unless `allow_local_tools = true`).
    pub fn register_container_tools(&self) {
        self.register_dev_tools();
    }

    /// Get tool definitions filtered by domain.
    pub async fn tool_definitions_for_domain(&self, domain: ToolDomain) -> Vec<ToolDefinition> {
        self.tools
            .read()
            .await
            .values()
            .filter(|tool| tool.domain() == domain)
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect()
    }

    /// Register development tools for building software.
    ///
    /// These tools provide shell access, file operations, and code editing
    /// capabilities needed for the software builder. Call this after
    /// `register_builtin_tools()` to enable code generation features.
    pub fn register_dev_tools(&self) {
        self.register_sync(Arc::new(ShellTool::new()));
        self.register_sync(Arc::new(ReadFileTool::new()));
        self.register_sync(Arc::new(WriteFileTool::new()));
        self.register_sync(Arc::new(ListDirTool::new()));
        self.register_sync(Arc::new(ApplyPatchTool::new()));

        tracing::debug!("Registered 5 development tools");
    }

    /// Register memory tools with a workspace.
    ///
    /// Memory tools require a workspace for persistence. Call this after
    /// `register_builtin_tools()` if you have a workspace available.
    pub fn register_memory_tools(&self, workspace: Arc<Workspace>) {
        self.register_sync(Arc::new(MemorySearchTool::new(Arc::clone(&workspace))));
        self.register_sync(Arc::new(MemoryWriteTool::new(Arc::clone(&workspace))));
        self.register_sync(Arc::new(MemoryReadTool::new(Arc::clone(&workspace))));
        self.register_sync(Arc::new(MemoryTreeTool::new(workspace)));

        tracing::debug!("Registered 4 memory tools");
    }

    /// Register job management tools.
    ///
    /// Job tools allow the LLM to create, list, check status, and cancel jobs.
    /// When sandbox deps are provided, `create_job` automatically delegates to
    /// Docker containers. Otherwise it dispatches via the Scheduler (which
    /// persists to DB and spawns a worker).
    #[allow(clippy::too_many_arguments)]
    pub fn register_job_tools(
        &self,
        context_manager: Arc<ContextManager>,
        scheduler_slot: Option<crate::tools::builtin::SchedulerSlot>,
        job_manager: Option<Arc<ContainerJobManager>>,
        store: Option<Arc<dyn Database>>,
        job_event_tx: Option<tokio::sync::broadcast::Sender<(uuid::Uuid, SseEvent)>>,
        inject_tx: Option<tokio::sync::mpsc::Sender<IncomingMessage>>,
        prompt_queue: Option<PromptQueue>,
        secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    ) {
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

        // Base tools: create, list, status, cancel
        let mut job_tool_count = 4;

        // Register event reader if store is available
        if let Some(store) = store {
            self.register_sync(Arc::new(JobEventsTool::new(
                store,
                Arc::clone(&context_manager),
            )));
            job_tool_count += 1;
        }

        // Register prompt tool if queue is available
        if let Some(pq) = prompt_queue {
            self.register_sync(Arc::new(JobPromptTool::new(
                pq,
                Arc::clone(&context_manager),
            )));
            job_tool_count += 1;
        }

        tracing::debug!("Registered {} job management tools", job_tool_count);
    }

    /// Register secret management tools (list, delete).
    ///
    /// These allow the LLM to persist API keys and tokens encrypted in the database.
    /// Values are never returned to the LLM; only names and metadata are exposed.
    pub fn register_secrets_tools(
        &self,
        store: Arc<dyn crate::secrets::SecretsStore + Send + Sync>,
    ) {
        use crate::tools::builtin::{SecretDeleteTool, SecretListTool};
        self.register_sync(Arc::new(SecretListTool::new(Arc::clone(&store))));
        self.register_sync(Arc::new(SecretDeleteTool::new(store)));
        tracing::debug!("Registered 2 secret management tools (list, delete)");
    }

    /// Register extension management tools (search, install, auth, activate, list, remove).
    ///
    /// These allow the LLM to manage MCP servers and WASM tools through conversation.
    pub fn register_extension_tools(&self, manager: Arc<ExtensionManager>) {
        self.register_sync(Arc::new(ToolSearchTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolInstallTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolAuthTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolActivateTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolListTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolRemoveTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolUpgradeTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ExtensionInfoTool::new(manager)));
        tracing::debug!("Registered 8 extension management tools");
    }

    /// Register skill management tools (list, search, install, remove).
    ///
    /// These allow the LLM to manage prompt-level skills through conversation.
    pub fn register_skill_tools(
        &self,
        registry: Arc<std::sync::RwLock<SkillRegistry>>,
        catalog: Arc<SkillCatalog>,
    ) {
        self.register_sync(Arc::new(SkillListTool::new(Arc::clone(&registry))));
        self.register_sync(Arc::new(SkillSearchTool::new(
            Arc::clone(&registry),
            Arc::clone(&catalog),
        )));
        self.register_sync(Arc::new(SkillInstallTool::new(
            Arc::clone(&registry),
            Arc::clone(&catalog),
        )));
        self.register_sync(Arc::new(SkillRemoveTool::new(registry)));
        tracing::debug!("Registered 4 skill management tools");
    }

    /// Register routine management tools.
    ///
    /// These allow the LLM to create, list, update, delete, and view history
    /// of routines (scheduled and event-driven tasks).
    pub fn register_routine_tools(&self, store: Arc<dyn Database>, engine: Arc<RoutineEngine>) {
        use crate::tools::builtin::{
            EventEmitTool, RoutineCreateTool, RoutineDeleteTool, RoutineFireTool,
            RoutineHistoryTool, RoutineListTool, RoutineUpdateTool,
        };
        self.register_sync(Arc::new(RoutineCreateTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineListTool::new(Arc::clone(&store))));
        self.register_sync(Arc::new(RoutineUpdateTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineDeleteTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineFireTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineHistoryTool::new(store)));
        self.register_sync(Arc::new(EventEmitTool::new(engine)));
        tracing::debug!("Registered 7 routine management tools");
    }

    /// Register message tool for sending messages to channels.
    pub async fn register_message_tools(
        &self,
        channel_manager: Arc<crate::channels::ChannelManager>,
    ) {
        use crate::tools::builtin::MessageTool;
        let tool = Arc::new(MessageTool::new(channel_manager));
        *self.message_tool.write().await = Some(Arc::clone(&tool));
        self.tools
            .write()
            .await
            .insert(tool.name().to_string(), tool as Arc<dyn Tool>);
        self.builtin_names
            .write()
            .await
            .insert("message".to_string());
        tracing::debug!("Registered message tool");
    }

    /// Set the default channel and target for the message tool.
    /// Call this before each agent turn with the current conversation's context.
    pub async fn set_message_tool_context(&self, channel: Option<String>, target: Option<String>) {
        if let Some(tool) = self.message_tool.read().await.as_ref() {
            tool.set_context(channel, target).await;
        }
    }

    /// Register image generation and editing tools.
    ///
    /// These tools allow the LLM to generate and edit images using cloud APIs.
    /// Requires an API base URL, API key, and model name for the image generation backend.
    pub fn register_image_tools(
        &self,
        api_base_url: String,
        api_key: String,
        gen_model: String,
        base_dir: Option<std::path::PathBuf>,
    ) {
        use crate::tools::builtin::{ImageEditTool, ImageGenerateTool};
        self.register_sync(Arc::new(ImageGenerateTool::new(
            api_base_url.clone(),
            api_key.clone(),
            gen_model.clone(),
        )));
        self.register_sync(Arc::new(ImageEditTool::new(
            api_base_url,
            api_key,
            gen_model,
            base_dir,
        )));
        tracing::debug!("Registered 2 image tools (generate, edit)");
    }

    /// Register vision/image analysis tools.
    ///
    /// These tools allow the LLM to analyze images using a vision-capable model.
    pub fn register_vision_tools(
        &self,
        api_base_url: String,
        api_key: String,
        vision_model: String,
        base_dir: Option<std::path::PathBuf>,
    ) {
        use crate::tools::builtin::ImageAnalyzeTool;
        self.register_sync(Arc::new(ImageAnalyzeTool::new(
            api_base_url,
            api_key,
            vision_model,
            base_dir,
        )));
        tracing::debug!("Registered 1 vision tool (analyze)");
    }

    /// Register the software builder tool.
    ///
    /// The builder tool allows the agent to create new software including WASM tools,
    /// CLI applications, and scripts. It uses an LLM-driven iterative build loop.
    ///
    /// This also registers the dev tools (shell, file operations) needed by the builder.
    pub async fn register_builder_tool(
        self: &Arc<Self>,
        llm: Arc<dyn LlmProvider>,
        config: Option<BuilderConfig>,
    ) {
        // First register dev tools needed by the builder
        self.register_dev_tools();

        // Create the builder (arg order: config, llm, tools)
        let builder = Arc::new(LlmSoftwareBuilder::new(
            config.unwrap_or_default(),
            llm,
            Arc::clone(self),
        ));

        // Register the protected build_software tool through the built-in path.
        self.register_sync(Arc::new(BuildSoftwareTool::new(builder)));

        tracing::debug!("Registered software builder tool");
    }
}
