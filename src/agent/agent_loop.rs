//! Main agent loop.
//!
//! Contains the `Agent` struct, `AgentDeps`, and the core event loop (`run`).
//! The heavy lifting is delegated to sibling modules:
//!
//! - `dispatcher` - Tool dispatch (agentic loop, tool execution)
//! - `commands` - System commands and job handlers
//! - `thread_ops` - Thread/session operations (user input, undo, approval, persistence)
//! - `background_tasks` - Spawning of self-repair, heartbeat, and routine tasks
//! - `notifications` - Outbound hook filtering and repair notification forwarding

use std::sync::Arc;

use futures::StreamExt;

use crate::agent::context_monitor::ContextMonitor;
use crate::agent::session_manager::SessionManager;
use crate::agent::{Router, Scheduler};
use crate::channels::{ChannelManager, IncomingMessage, OutgoingResponse};
use crate::config::{AgentConfig, HeartbeatConfig, RoutineConfig, SkillsConfig};
use crate::context::ContextManager;
use crate::db::Database;
use crate::error::Error;
use crate::extensions::ExtensionManager;
use crate::hooks::HookRegistry;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

mod background_tasks;
mod notifications;
#[cfg(test)]
mod tests;

use notifications::apply_before_outbound_hooks;

/// Core dependencies for the agent.
///
/// Bundles the shared components to reduce argument count.
pub struct AgentDeps {
    /// Database backend for persisted threads and jobs. `None` when no database
    /// is configured, in which case state is not persisted.
    pub store: Option<Arc<dyn Database>>,
    /// Main LLM provider used for agent turns.
    pub llm: Arc<dyn LlmProvider>,
    /// Cheap/fast LLM for lightweight tasks (heartbeat, routing, evaluation).
    /// Falls back to the main `llm` if None.
    pub cheap_llm: Option<Arc<dyn LlmProvider>>,
    /// Safety layer that sanitizes tool output and validates inbound content.
    pub safety: Arc<SafetyLayer>,
    /// Registry of built-in and extension tools the agent may call.
    pub tools: Arc<ToolRegistry>,
    /// Workspace used for memory and indexed documents. `None` disables both.
    pub workspace: Option<Arc<Workspace>>,
    /// Lifecycle manager for installed extensions. `None` disables extension
    /// activation and auth flows.
    pub extension_manager: Option<Arc<ExtensionManager>>,
    /// Loaded skills, behind a lock so the set can change at runtime.
    pub skill_registry: Option<Arc<std::sync::RwLock<SkillRegistry>>>,
    /// Remote skill catalogue used for searching and installing skills.
    pub skill_catalog: Option<Arc<crate::skills::catalog::SkillCatalog>>,
    /// Skill discovery and selection settings.
    pub skills_config: SkillsConfig,
    /// Hook registry consulted at lifecycle points such as before outbound messages.
    pub hooks: Arc<HookRegistry>,
    /// Cost enforcement guardrails (daily budget, hourly rate limits).
    pub cost_guard: Arc<crate::agent::cost_guard::CostGuard>,
    /// SSE broadcast sender for live job event streaming to the web gateway.
    pub sse_tx: Option<tokio::sync::broadcast::Sender<crate::channels::web::types::SseEvent>>,
    /// HTTP interceptor for trace recording/replay.
    pub http_interceptor: Option<Arc<dyn crate::llm::recording::HttpInterceptor>>,
    /// Audio transcription middleware for voice messages.
    pub transcription: Option<Arc<crate::transcription::TranscriptionMiddleware>>,
    /// Document text extraction middleware for PDF, DOCX, PPTX, etc.
    pub document_extraction: Option<Arc<crate::document_extraction::DocumentExtractionMiddleware>>,
}

/// The main agent that coordinates all components.
pub struct Agent {
    pub(super) config: AgentConfig,
    pub(super) deps: AgentDeps,
    pub(super) channels: Arc<ChannelManager>,
    pub(super) context_manager: Arc<ContextManager>,
    pub(super) scheduler: Arc<Scheduler>,
    pub(super) router: Router,
    pub(super) session_manager: Arc<SessionManager>,
    pub(super) context_monitor: ContextMonitor,
    pub(super) heartbeat_config: Option<HeartbeatConfig>,
    pub(super) hygiene_config: Option<crate::config::HygieneConfig>,
    pub(super) routine_config: Option<RoutineConfig>,
    /// Optional slot to expose the routine engine to the gateway for manual triggering.
    pub(super) routine_engine_slot:
        Option<Arc<tokio::sync::RwLock<Option<Arc<crate::agent::routine_engine::RoutineEngine>>>>>,
}

/// Wait for the next incoming message, or `None` on Ctrl+C or when every channel stream ends.
async fn next_message(
    message_stream: &mut crate::channels::MessageStream,
) -> Option<IncomingMessage> {
    tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            tracing::debug!("Ctrl+C received, shutting down...");
            None
        }
        msg = message_stream.next() => {
            if msg.is_none() {
                tracing::debug!("All channel streams ended, shutting down...");
            }
            msg
        }
    }
}

/// Fire routines whose event triggers match the message (cheap in-memory regex, async if matched).
async fn fire_event_triggers(
    engine: Option<&crate::agent::routine_engine::RoutineEngine>,
    message: &IncomingMessage,
) {
    let Some(engine) = engine else {
        return;
    };
    let fired = engine.check_event_triggers(message).await;
    if fired > 0 {
        tracing::debug!("Fired {} event-triggered routines", fired);
    }
}

impl Agent {
    /// Create a new agent.
    ///
    /// Optionally accepts pre-created `ContextManager` and `SessionManager` for sharing
    /// with external components (job tools, web gateway). Creates new ones if not provided.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AgentConfig,
        deps: AgentDeps,
        channels: Arc<ChannelManager>,
        heartbeat_config: Option<HeartbeatConfig>,
        hygiene_config: Option<crate::config::HygieneConfig>,
        routine_config: Option<RoutineConfig>,
        context_manager: Option<Arc<ContextManager>>,
        session_manager: Option<Arc<SessionManager>>,
    ) -> Self {
        let context_manager = context_manager
            .unwrap_or_else(|| Arc::new(ContextManager::new(config.max_parallel_jobs)));

        let session_manager = session_manager.unwrap_or_else(|| Arc::new(SessionManager::new()));

        let mut scheduler = Scheduler::new(
            config.clone(),
            context_manager.clone(),
            deps.llm.clone(),
            deps.safety.clone(),
            deps.tools.clone(),
            deps.store.clone(),
            deps.hooks.clone(),
        );
        if let Some(ref tx) = deps.sse_tx {
            scheduler.set_sse_sender(tx.clone());
        }
        if let Some(ref interceptor) = deps.http_interceptor {
            scheduler.set_http_interceptor(Arc::clone(interceptor));
        }
        let scheduler = Arc::new(scheduler);

        Self {
            config,
            deps,
            channels,
            context_manager,
            scheduler,
            router: Router::new(),
            session_manager,
            context_monitor: ContextMonitor::new(),
            heartbeat_config,
            hygiene_config,
            routine_config,
            routine_engine_slot: None,
        }
    }

    /// Run the agent main loop.
    pub async fn run(self) -> Result<(), Error> {
        // Start channels
        let mut message_stream = self.channels.start_all().await?;

        // Start self-repair task with notification forwarding
        let self_repair = self.spawn_self_repair();

        // Spawn session pruning task
        let pruning_handle = self.spawn_session_pruning();

        // Spawn heartbeat if enabled
        let heartbeat_handle = self.spawn_heartbeat().await;

        // Spawn routine engine if enabled
        let routine = self.spawn_routine_engine().await;

        // Extract engine ref for use in message loop
        let routine_engine_for_loop = routine.as_ref().map(|r| Arc::clone(&r.engine));

        tracing::debug!("Agent {} ready and listening", self.config.name);
        self.run_message_loop(&mut message_stream, routine_engine_for_loop.as_deref())
            .await;

        tracing::debug!("Agent shutting down...");
        self.stop_background_tasks(self_repair, pruning_handle, heartbeat_handle, routine)
            .await;
        self.shutdown_channels().await
    }

    /// Handle messages until the stream ends, Ctrl+C arrives or a shutdown command breaks the loop.
    async fn run_message_loop(
        &self,
        message_stream: &mut crate::channels::MessageStream,
        routine_engine: Option<&crate::agent::routine_engine::RoutineEngine>,
    ) {
        while let Some(mut message) = next_message(message_stream).await {
            self.preprocess_message(&mut message).await;

            if self.respond_to_message(&message).await.is_break() {
                break;
            }

            // Check event triggers (cheap in-memory regex, fires async if matched)
            fire_event_triggers(routine_engine, &message).await;
        }
    }

    /// Clear the routine engine slot, stop all scheduled jobs and shut the channels down.
    async fn shutdown_channels(&self) -> Result<(), Error> {
        if let Some(ref slot) = self.routine_engine_slot {
            *slot.write().await = None;
        }
        self.scheduler.stop_all().await;
        self.channels.shutdown_all().await?;
        Ok(())
    }

    /// Apply the attachment middleware (transcription, document extraction) and index extracted text.
    async fn preprocess_message(&self, message: &mut IncomingMessage) {
        // Apply transcription middleware to audio attachments
        if let Some(ref transcription) = self.deps.transcription {
            transcription.process(message).await;
        }

        // Apply document extraction middleware to document attachments
        if let Some(ref doc_extraction) = self.deps.document_extraction {
            doc_extraction.process(message).await;
        }

        // Store successfully extracted document text in workspace for indexing
        if let Some(workspace) = self.workspace() {
            super::thread_ops::store_extracted_documents(workspace, message).await;
        }
    }

    /// Handle one message and send the outcome to its channel; `Break` means shut down.
    async fn respond_to_message(&self, message: &IncomingMessage) -> std::ops::ControlFlow<()> {
        match self.handle_message(message).await {
            Ok(Some(response)) if !response.is_empty() => {
                self.send_response_through_hooks(message, response).await;
            }
            Ok(Some(empty)) => {
                // Empty response, nothing to send (e.g. approval handled via send_status)
                tracing::debug!(
                    channel = %message.channel,
                    user = %message.user_id,
                    empty_len = empty.len(),
                    "Suppressed empty response (not sent to channel)"
                );
            }
            Ok(None) => {
                // Shutdown signal received (/quit, /exit, /shutdown)
                tracing::debug!("Shutdown command received, exiting...");
                return std::ops::ControlFlow::Break(());
            }
            Err(e) => {
                tracing::error!("Error handling message: {}", e);
                self.send_error_response(message, &e).await;
            }
        }
        std::ops::ControlFlow::Continue(())
    }

    /// Run the outbound hooks over a response and send what they allow to the channel.
    async fn send_response_through_hooks(&self, message: &IncomingMessage, response: String) {
        if let Some(response) = apply_before_outbound_hooks(
            self.hooks(),
            notifications::OutboundRoute {
                user_id: &message.user_id,
                channel: &message.channel,
                thread_id: message.thread_id.as_deref(),
            },
            OutgoingResponse::text(response),
        )
        .await
            && let Err(e) = self.channels.respond(message, response).await
        {
            tracing::error!(
                channel = %message.channel,
                error = %e,
                "Failed to send response to channel"
            );
        }
    }

    /// Tell the channel that handling its message failed.
    async fn send_error_response(&self, message: &IncomingMessage, error: &Error) {
        if let Err(send_err) = self
            .channels
            .respond(message, OutgoingResponse::text(format!("Error: {}", error)))
            .await
        {
            tracing::error!(
                channel = %message.channel,
                error = %send_err,
                "Failed to send error response to channel"
            );
        }
    }

    /// Stop the self-repair, pruning, heartbeat and routine tasks started by `run`.
    async fn stop_background_tasks(
        &self,
        self_repair: background_tasks::SelfRepairRuntime,
        pruning_handle: tokio::task::JoinHandle<()>,
        heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
        routine: Option<background_tasks::RoutineHandles>,
    ) {
        let _ = self_repair.shutdown_tx.send(());
        if let Err(error) = self_repair.repair_handle.await {
            tracing::debug!("Repair task join finished with error: {}", error);
        }
        if let Err(error) = self_repair.notify_handle.await {
            tracing::debug!("Repair notification task exited with error: {}", error);
        }
        pruning_handle.abort();
        if let Some(handle) = heartbeat_handle {
            handle.abort();
        }
        if let Some(r) = routine {
            r.notify_forwarder.abort();
            r.cron_handle.abort();
        }
    }
}
