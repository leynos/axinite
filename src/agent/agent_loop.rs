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
use crate::channels::{ChannelManager, OutgoingResponse};
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
    pub store: Option<Arc<dyn Database>>,
    pub llm: Arc<dyn LlmProvider>,
    /// Cheap/fast LLM for lightweight tasks (heartbeat, routing, evaluation).
    /// Falls back to the main `llm` if None.
    pub cheap_llm: Option<Arc<dyn LlmProvider>>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub workspace: Option<Arc<Workspace>>,
    pub extension_manager: Option<Arc<ExtensionManager>>,
    pub skill_registry: Option<Arc<std::sync::RwLock<SkillRegistry>>>,
    pub skill_catalog: Option<Arc<crate::skills::catalog::SkillCatalog>>,
    pub skills_config: SkillsConfig,
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

        // Main message loop
        tracing::debug!("Agent {} ready and listening", self.config.name);

        loop {
            let message = tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => {
                    tracing::debug!("Ctrl+C received, shutting down...");
                    break;
                }
                msg = message_stream.next() => {
                    match msg {
                        Some(m) => m,
                        None => {
                            tracing::debug!("All channel streams ended, shutting down...");
                            break;
                        }
                    }
                }
            };

            // Apply transcription middleware to audio attachments
            let mut message = message;
            if let Some(ref transcription) = self.deps.transcription {
                transcription.process(&mut message).await;
            }

            // Apply document extraction middleware to document attachments
            if let Some(ref doc_extraction) = self.deps.document_extraction {
                doc_extraction.process(&mut message).await;
            }

            // Store successfully extracted document text in workspace for indexing
            if let Some(workspace) = self.workspace() {
                super::thread_ops::store_extracted_documents(workspace, &message).await;
            }

            match self.handle_message(&message).await {
                Ok(Some(response)) if !response.is_empty() => {
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
                        && let Err(e) = self.channels.respond(&message, response).await
                    {
                        tracing::error!(
                            channel = %message.channel,
                            error = %e,
                            "Failed to send response to channel"
                        );
                    }
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
                    break;
                }
                Err(e) => {
                    tracing::error!("Error handling message: {}", e);
                    if let Err(send_err) = self
                        .channels
                        .respond(&message, OutgoingResponse::text(format!("Error: {}", e)))
                        .await
                    {
                        tracing::error!(
                            channel = %message.channel,
                            error = %send_err,
                            "Failed to send error response to channel"
                        );
                    }
                }
            }

            // Check event triggers (cheap in-memory regex, fires async if matched)
            if let Some(ref engine) = routine_engine_for_loop {
                let fired = engine.check_event_triggers(&message).await;
                if fired > 0 {
                    tracing::debug!("Fired {} event-triggered routines", fired);
                }
            }
        }

        // Cleanup
        tracing::debug!("Agent shutting down...");
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
        if let Some(ref slot) = self.routine_engine_slot {
            *slot.write().await = None;
        }
        self.scheduler.stop_all().await;
        self.channels.shutdown_all().await?;

        Ok(())
    }
}
