//! Main agent loop.
//!
//! Contains the `Agent` struct, `AgentDeps`, and the core event loop (`run`).
//! The heavy lifting is delegated to sibling modules:
//!
//! - `dispatcher` - Tool dispatch (agentic loop, tool execution)
//! - `commands` - System commands and job handlers
//! - `thread_ops` - Thread/session operations (user input, undo, approval, persistence)

use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::agent::context_monitor::ContextMonitor;
use crate::agent::heartbeat::spawn_heartbeat;
use crate::agent::routine_engine::{RoutineEngine, spawn_cron_ticker};
use crate::agent::self_repair::{
    DefaultSelfRepair, RepairNotification, RepairNotificationRoute, RepairTask,
};
use crate::agent::session_manager::SessionManager;
use crate::agent::submission::{Submission, SubmissionParser, SubmissionResult};
use crate::agent::{HeartbeatConfig as AgentHeartbeatConfig, Router, Scheduler};
use crate::channels::{ChannelManager, IncomingMessage, OutgoingResponse, StatusUpdate};
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

#[cfg(test)]
pub(crate) use super::dispatcher::truncate_for_preview;

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

struct SelfRepairRuntime {
    shutdown_tx: oneshot::Sender<()>,
    repair_handle: JoinHandle<()>,
    notify_handle: JoinHandle<()>,
}

struct RoutineHandles {
    cron_handle: JoinHandle<()>,
    engine: Arc<RoutineEngine>,
}

/// Reserved user ID for system-generated repair notifications.
const SYSTEM_USER_ID: &str = "default";

pub(super) async fn apply_before_outbound_hooks(
    hooks: &Arc<HookRegistry>,
    user_id: &str,
    channel: &str,
    thread_id: Option<&str>,
    response: OutgoingResponse,
) -> Option<OutgoingResponse> {
    let event = crate::hooks::HookEvent::Outbound {
        user_id: user_id.to_string(),
        channel: channel.to_string(),
        content: response.content.clone(),
        thread_id: thread_id.map(str::to_string),
    };
    match hooks.run(&event).await {
        Err(crate::hooks::HookError::Rejected { reason }) => {
            tracing::warn!("BeforeOutbound hook blocked response: {}", reason);
            None
        }
        Err(err) => {
            tracing::warn!("BeforeOutbound hook failed open: {}", err);
            Some(response)
        }
        Ok(crate::hooks::HookOutcome::Continue {
            modified: Some(new_content),
        }) => Some(OutgoingResponse {
            content: new_content,
            ..response
        }),
        Ok(crate::hooks::HookOutcome::Continue { modified: None }) => Some(response),
        Ok(crate::hooks::HookOutcome::Reject { reason }) => {
            tracing::warn!("BeforeOutbound hook blocked response: {}", reason);
            None
        }
    }
}

pub(super) async fn forward_repair_notification(
    channels: &Arc<ChannelManager>,
    hooks: &Arc<HookRegistry>,
    notification: RepairNotification,
) {
    match notification.route {
        RepairNotificationRoute::BroadcastAll { user_id } => {
            let response = OutgoingResponse::text(format!("Self-Repair: {}", notification.message));
            for channel in channels.channel_names().await {
                let Some(filtered_response) =
                    apply_before_outbound_hooks(hooks, &user_id, &channel, None, response.clone())
                        .await
                else {
                    continue;
                };
                if let Err(error) = channels
                    .broadcast(&channel, &user_id, filtered_response)
                    .await
                {
                    tracing::warn!(
                        "Failed to broadcast self-repair notification to {}: {}",
                        channel,
                        error
                    );
                }
            }
        }
        RepairNotificationRoute::Broadcast { channel, user_id } => {
            let response = OutgoingResponse::text(format!("Self-Repair: {}", notification.message));
            let Some(response) =
                apply_before_outbound_hooks(hooks, &user_id, &channel, None, response).await
            else {
                return;
            };
            if let Err(error) = channels.broadcast(&channel, &user_id, response).await {
                tracing::warn!(
                    "Failed to broadcast self-repair notification to {}: {}",
                    channel,
                    error
                );
            }
        }
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

    /// Set the routine engine slot for exposing the engine to the gateway.
    pub fn set_routine_engine_slot(
        &mut self,
        slot: Arc<tokio::sync::RwLock<Option<Arc<crate::agent::routine_engine::RoutineEngine>>>>,
    ) {
        self.routine_engine_slot = Some(slot);
    }

    // Convenience accessors

    /// Get the scheduler (for external wiring, e.g. CreateJobTool).
    pub fn scheduler(&self) -> Arc<Scheduler> {
        Arc::clone(&self.scheduler)
    }

    pub(super) fn store(&self) -> Option<&Arc<dyn Database>> {
        self.deps.store.as_ref()
    }

    pub(super) fn llm(&self) -> &Arc<dyn LlmProvider> {
        &self.deps.llm
    }

    /// Get the cheap/fast LLM provider, falling back to the main one.
    pub(super) fn cheap_llm(&self) -> &Arc<dyn LlmProvider> {
        self.deps.cheap_llm.as_ref().unwrap_or(&self.deps.llm)
    }

    pub(super) fn safety(&self) -> &Arc<SafetyLayer> {
        &self.deps.safety
    }

    pub(super) fn tools(&self) -> &Arc<ToolRegistry> {
        &self.deps.tools
    }

    pub(super) fn workspace(&self) -> Option<&Arc<Workspace>> {
        self.deps.workspace.as_ref()
    }

    pub(super) fn hooks(&self) -> &Arc<HookRegistry> {
        &self.deps.hooks
    }

    pub(super) fn cost_guard(&self) -> &Arc<crate::agent::cost_guard::CostGuard> {
        &self.deps.cost_guard
    }

    pub(super) fn skill_registry(&self) -> Option<&Arc<std::sync::RwLock<SkillRegistry>>> {
        self.deps.skill_registry.as_ref()
    }

    pub(super) fn skill_catalog(&self) -> Option<&Arc<crate::skills::catalog::SkillCatalog>> {
        self.deps.skill_catalog.as_ref()
    }

    fn spawn_self_repair(&self) -> SelfRepairRuntime {
        let mut repair = DefaultSelfRepair::new(
            self.context_manager.clone(),
            self.config.stuck_threshold,
            self.config.max_repair_attempts,
        );
        if let Some(store) = self.store() {
            repair = repair.with_store(Arc::clone(store));
        }
        let repair = Arc::new(repair);
        let repair_interval = self.config.repair_check_interval;
        let repair_channels = self.channels.clone();
        let repair_hooks = Arc::clone(self.hooks());
        let (repair_shutdown_tx, repair_shutdown_rx) = oneshot::channel();
        let (repair_notify_tx, mut repair_notify_rx) = mpsc::channel::<RepairNotification>(16);
        let repair_task = RepairTask::new(repair, repair_interval, repair_shutdown_rx)
            .with_notification_tx(
                repair_notify_tx,
                RepairNotificationRoute::BroadcastAll {
                    // System-level repair notices target the reserved system user.
                    user_id: SYSTEM_USER_ID.to_string(),
                },
            );
        let repair_handle = tokio::spawn(repair_task.run());
        let notify_handle = tokio::spawn(async move {
            while let Some(notification) = repair_notify_rx.recv().await {
                forward_repair_notification(&repair_channels, &repair_hooks, notification).await;
            }
        });

        SelfRepairRuntime {
            shutdown_tx: repair_shutdown_tx,
            repair_handle,
            notify_handle,
        }
    }

    fn spawn_session_pruning(&self) -> JoinHandle<()> {
        let session_mgr = self.session_manager.clone();
        let session_idle_timeout = self.config.session_idle_timeout;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600)); // Every 10 min
            interval.tick().await; // Skip immediate first tick
            loop {
                interval.tick().await;
                session_mgr.prune_stale_sessions(session_idle_timeout).await;
            }
        })
    }

    async fn spawn_heartbeat(&self) -> Option<JoinHandle<()>> {
        let hb_config = self.heartbeat_config.as_ref()?;
        if !hb_config.enabled {
            return None;
        }
        let workspace = match self.workspace() {
            Some(w) => w,
            None => {
                tracing::warn!("Heartbeat enabled but no workspace available");
                return None;
            }
        };
        let mut config = AgentHeartbeatConfig::default()
            .with_interval(std::time::Duration::from_secs(hb_config.interval_secs));
        config.quiet_hours_start = hb_config.quiet_hours_start;
        config.quiet_hours_end = hb_config.quiet_hours_end;
        config.timezone = hb_config
            .timezone
            .clone()
            .or_else(|| Some(self.config.default_timezone.clone()));
        if let (Some(user), Some(channel)) = (&hb_config.notify_user, &hb_config.notify_channel) {
            config = config.with_notify(user, channel);
        }

        // Set up notification channel
        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<OutgoingResponse>(16);

        // Spawn notification forwarder that routes through channel manager
        let notify_channel = hb_config.notify_channel.clone();
        let notify_user = hb_config.notify_user.clone();
        let channels = self.channels.clone();
        tokio::spawn(async move {
            while let Some(response) = notify_rx.recv().await {
                let user = notify_user.as_deref().unwrap_or("default");

                // Try the configured channel first, fall back to
                // broadcasting on all channels.
                let targeted_ok = if let Some(ref channel) = notify_channel {
                    channels
                        .broadcast(channel, user, response.clone())
                        .await
                        .is_ok()
                } else {
                    false
                };

                if !targeted_ok {
                    let results = channels.broadcast_all(user, response).await;
                    for (ch, result) in results {
                        if let Err(e) = result {
                            tracing::warn!("Failed to broadcast heartbeat to {}: {}", ch, e);
                        }
                    }
                }
            }
        });

        let hygiene = self
            .hygiene_config
            .as_ref()
            .map(|h| h.to_workspace_config())
            .unwrap_or_default();

        Some(spawn_heartbeat(
            config,
            hygiene,
            workspace.clone(),
            self.cheap_llm().clone(),
            Some(notify_tx),
            self.store().map(Arc::clone),
        ))
    }

    async fn spawn_routine_engine(&self) -> Option<RoutineHandles> {
        let rt_config = self.routine_config.as_ref()?;
        if !rt_config.enabled {
            return None;
        }
        let (store, workspace) = match (self.store(), self.workspace()) {
            (Some(s), Some(w)) => (s, w),
            _ => {
                tracing::warn!("Routines enabled but store/workspace not available");
                return None;
            }
        };
        // Set up notification channel (same pattern as heartbeat)
        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<OutgoingResponse>(32);

        let engine = Arc::new(RoutineEngine::new(
            rt_config.clone(),
            Arc::clone(store),
            self.llm().clone(),
            Arc::clone(workspace),
            notify_tx,
            Some(self.scheduler.clone()),
            self.tools().clone(),
            self.safety().clone(),
        ));

        // Register routine tools
        self.deps
            .tools
            .register_routine_tools(Arc::clone(store), Arc::clone(&engine));

        // Load initial event cache
        engine.refresh_event_cache().await;

        // Spawn notification forwarder (mirrors heartbeat pattern)
        let channels = self.channels.clone();
        tokio::spawn(async move {
            while let Some(response) = notify_rx.recv().await {
                let user = response
                    .metadata
                    .get("notify_user")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                let notify_channel = response
                    .metadata
                    .get("notify_channel")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Try the configured channel first, fall back to
                // broadcasting on all channels.
                let targeted_ok = if let Some(ref channel) = notify_channel {
                    channels
                        .broadcast(channel, &user, response.clone())
                        .await
                        .is_ok()
                } else {
                    false
                };

                if !targeted_ok {
                    let results = channels.broadcast_all(&user, response).await;
                    for (ch, result) in results {
                        if let Err(e) = result {
                            tracing::warn!(
                                "Failed to broadcast routine notification to {}: {}",
                                ch,
                                e
                            );
                        }
                    }
                }
            }
        });

        // Spawn cron ticker
        let cron_interval = std::time::Duration::from_secs(rt_config.cron_check_interval_secs);
        let cron_handle = spawn_cron_ticker(Arc::clone(&engine), cron_interval);

        // Store engine reference for event trigger checking
        let engine_ref = Arc::clone(&engine);
        // `run()` consumes self, so cloning the engine into a local keeps it
        // available for the message loop without changing ownership semantics.

        // Expose engine to gateway for manual triggering
        if let Some(ref slot) = self.routine_engine_slot {
            *slot.write().await = Some(Arc::clone(&engine));
        }

        tracing::debug!(
            "Routines enabled: cron ticker every {}s, max {} concurrent",
            rt_config.cron_check_interval_secs,
            rt_config.max_concurrent_routines
        );

        Some(RoutineHandles {
            cron_handle,
            engine: engine_ref,
        })
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
                        &message.user_id,
                        &message.channel,
                        message.thread_id.as_deref(),
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
            r.cron_handle.abort();
        }
        self.scheduler.stop_all().await;
        self.channels.shutdown_all().await?;

        Ok(())
    }

    async fn handle_message(&self, message: &IncomingMessage) -> Result<Option<String>, Error> {
        // Log at info level only for tracking without exposing PII (user_id can be a phone number)
        tracing::info!(message_id = %message.id, "Processing message");

        // Log sensitive details at debug level for troubleshooting
        tracing::debug!(
            message_id = %message.id,
            user_id = %message.user_id,
            channel = %message.channel,
            thread_id = ?message.thread_id,
            "Message details"
        );

        // Set message tool context for this turn (current channel and target)
        // For Signal, use signal_target from metadata (group:ID or phone number),
        // otherwise fall back to user_id
        let target = message
            .metadata
            .get("signal_target")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| message.user_id.clone());
        self.tools()
            .set_message_tool_context(Some(message.channel.clone()), Some(target))
            .await;

        // Parse submission type first
        let mut submission = SubmissionParser::parse(&message.content);
        tracing::trace!(
            "[agent_loop] Parsed submission: {:?}",
            std::any::type_name_of_val(&submission)
        );

        // Hook: BeforeInbound — allow hooks to modify or reject user input
        if let Submission::UserInput { ref content } = submission {
            let event = crate::hooks::HookEvent::Inbound {
                user_id: message.user_id.clone(),
                channel: message.channel.clone(),
                content: content.clone(),
                thread_id: message.thread_id.clone(),
            };
            match self.hooks().run(&event).await {
                Err(crate::hooks::HookError::Rejected { reason }) => {
                    return Ok(Some(format!("[Message rejected: {}]", reason)));
                }
                Err(err) => {
                    return Ok(Some(format!("[Message blocked by hook policy: {}]", err)));
                }
                Ok(crate::hooks::HookOutcome::Continue {
                    modified: Some(new_content),
                }) => {
                    submission = Submission::UserInput {
                        content: new_content,
                    };
                }
                _ => {} // Continue, fail-open errors already logged in registry
            }
        }

        // Hydrate thread from DB if it's a historical thread not in memory
        if let Some(ref external_thread_id) = message.thread_id {
            tracing::trace!(
                message_id = %message.id,
                thread_id = %external_thread_id,
                "Hydrating thread from DB"
            );
            self.maybe_hydrate_thread(message, external_thread_id).await;
        }

        // Resolve session and thread
        tracing::debug!(
            message_id = %message.id,
            "Resolving session and thread"
        );
        let (session, thread_id) = self
            .session_manager
            .resolve_thread(
                &message.user_id,
                &message.channel,
                message.thread_id.as_deref(),
            )
            .await;
        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            "Resolved session and thread"
        );

        // Auth mode interception: if the thread is awaiting a token, route
        // the message directly to the credential store. Nothing touches
        // logs, turns, history, or compaction.
        let pending_auth = {
            let sess = session.lock().await;
            sess.threads
                .get(&thread_id)
                .and_then(|t| t.pending_auth.clone())
        };

        if let Some(pending) = pending_auth {
            match &submission {
                Submission::UserInput { content } => {
                    return self
                        .process_auth_token(message, &pending, content, session, thread_id)
                        .await;
                }
                _ => {
                    // Any control submission (interrupt, undo, etc.) cancels auth mode
                    let mut sess = session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&thread_id) {
                        thread.pending_auth = None;
                    }
                    // Fall through to normal handling
                }
            }
        }

        tracing::trace!(
            "Received message from {} on {} ({} chars)",
            message.user_id,
            message.channel,
            message.content.len()
        );

        // Process based on submission type
        let result = match submission {
            Submission::UserInput { content } => {
                self.process_user_input(message, session, thread_id, &content)
                    .await
            }
            Submission::SystemCommand { command, args } => {
                tracing::debug!(
                    "[agent_loop] SystemCommand: command={}, channel={}",
                    command,
                    message.channel
                );
                // Authorization checks (including restart channel check) are enforced in handle_system_command
                self.handle_system_command(&command, &args, &message.channel)
                    .await
            }
            Submission::Undo => self.process_undo(session, thread_id).await,
            Submission::Redo => self.process_redo(session, thread_id).await,
            Submission::Interrupt => self.process_interrupt(session, thread_id).await,
            Submission::Compact => self.process_compact(session, thread_id).await,
            Submission::Clear => self.process_clear(session, thread_id).await,
            Submission::NewThread => self.process_new_thread(message).await,
            Submission::Heartbeat => self.process_heartbeat().await,
            Submission::Summarize => self.process_summarize(session, thread_id).await,
            Submission::Suggest => self.process_suggest(session, thread_id).await,
            Submission::JobStatus { job_id } => {
                self.process_job_status(&message.user_id, job_id.as_deref())
                    .await
            }
            Submission::JobCancel { job_id } => {
                self.process_job_cancel(&message.user_id, &job_id).await
            }
            Submission::Quit => return Ok(None),
            Submission::SwitchThread { thread_id: target } => {
                self.process_switch_thread(message, target).await
            }
            Submission::Resume { checkpoint_id } => {
                self.process_resume(session, thread_id, checkpoint_id).await
            }
            Submission::ExecApproval {
                request_id,
                approved,
                always,
            } => {
                self.process_approval(
                    message,
                    session,
                    thread_id,
                    Some(request_id),
                    approved,
                    always,
                )
                .await
            }
            Submission::ApprovalResponse { approved, always } => {
                self.process_approval(message, session, thread_id, None, approved, always)
                    .await
            }
        };

        // Convert SubmissionResult to response string
        match result? {
            SubmissionResult::Response { content } => {
                // Suppress silent replies (e.g. from group chat "nothing to say" responses)
                if crate::llm::is_silent_reply(&content) {
                    tracing::debug!("Suppressing silent reply token");
                    Ok(None)
                } else {
                    Ok(Some(content))
                }
            }
            SubmissionResult::Ok { message } => Ok(message),
            SubmissionResult::Error { message } => Ok(Some(format!("Error: {}", message))),
            SubmissionResult::Interrupted => Ok(Some("Interrupted.".into())),
            SubmissionResult::NeedApproval {
                request_id,
                tool_name,
                description,
                parameters,
            } => {
                // Each channel renders the approval prompt via send_status.
                // Web gateway shows an inline card, REPL prints a formatted prompt, etc.
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::ApprovalNeeded {
                            request_id: request_id.to_string(),
                            tool_name,
                            description,
                            parameters,
                        },
                        &message.metadata,
                    )
                    .await;

                // Empty string signals the caller to skip respond() (no duplicate text)
                Ok(Some(String::new()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::mpsc;

    use super::{Agent, AgentDeps, truncate_for_preview};
    use crate::channels::{
        ChannelManager, IncomingMessage, MessageStream, NativeChannel, OutgoingResponse,
    };
    use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
    use crate::context::ContextManager;
    use crate::error::ChannelError;
    use crate::hooks::HookRegistry;
    use crate::safety::SafetyLayer;
    use crate::testing::StubLlm;
    use crate::tools::ToolRegistry;

    type BroadcastLog = Arc<Mutex<Vec<(String, OutgoingResponse)>>>;

    struct BroadcastCaptureChannel {
        name: String,
        rx: tokio::sync::Mutex<Option<mpsc::Receiver<IncomingMessage>>>,
        broadcasts: BroadcastLog,
    }

    impl BroadcastCaptureChannel {
        fn new(name: impl Into<String>) -> (Self, mpsc::Sender<IncomingMessage>, BroadcastLog) {
            let (tx, rx) = mpsc::channel(16);
            let broadcasts = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    name: name.into(),
                    rx: tokio::sync::Mutex::new(Some(rx)),
                    broadcasts: Arc::clone(&broadcasts),
                },
                tx,
                broadcasts,
            )
        }
    }

    impl NativeChannel for BroadcastCaptureChannel {
        fn name(&self) -> &str {
            &self.name
        }

        async fn start(&self) -> Result<MessageStream, ChannelError> {
            let rx = self
                .rx
                .lock()
                .await
                .take()
                .ok_or_else(|| ChannelError::StartupFailed {
                    name: self.name.clone(),
                    reason: "start() already called".to_string(),
                })?;
            Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
        }

        async fn respond(
            &self,
            _msg: &IncomingMessage,
            _response: OutgoingResponse,
        ) -> Result<(), ChannelError> {
            Ok(())
        }

        async fn broadcast(
            &self,
            user_id: &str,
            response: OutgoingResponse,
        ) -> Result<(), ChannelError> {
            self.broadcasts
                .lock()
                .expect("broadcast capture should not be poisoned")
                .push((user_id.to_string(), response));
            Ok(())
        }

        async fn health_check(&self) -> Result<(), ChannelError> {
            Ok(())
        }
    }

    fn make_test_agent(
        channels: Arc<ChannelManager>,
        context_manager: Arc<ContextManager>,
        repair_check_interval: Duration,
        stuck_threshold: Duration,
    ) -> Agent {
        let deps = AgentDeps {
            store: None,
            llm: Arc::new(StubLlm::new("ok")),
            cheap_llm: None,
            safety: Arc::new(SafetyLayer::new(&SafetyConfig {
                max_output_length: 100_000,
                injection_check_enabled: false,
            })),
            tools: Arc::new(ToolRegistry::new()),
            workspace: None,
            extension_manager: None,
            skill_registry: None,
            skill_catalog: None,
            skills_config: SkillsConfig::default(),
            hooks: Arc::new(HookRegistry::new()),
            cost_guard: Arc::new(crate::agent::cost_guard::CostGuard::new(
                crate::agent::cost_guard::CostGuardConfig::default(),
            )),
            sse_tx: None,
            http_interceptor: None,
            transcription: None,
            document_extraction: None,
        };

        Agent::new(
            AgentConfig {
                name: "test-agent".to_string(),
                max_parallel_jobs: 1,
                job_timeout: Duration::from_secs(60),
                stuck_threshold,
                repair_check_interval,
                max_repair_attempts: 2,
                use_planning: false,
                session_idle_timeout: Duration::from_secs(300),
                allow_local_tools: false,
                max_cost_per_day_cents: None,
                max_actions_per_hour: None,
                max_tool_iterations: 4,
                auto_approve_tools: false,
                default_timezone: "UTC".to_string(),
                max_tokens_per_job: 0,
            },
            deps,
            channels,
            None,
            None,
            None,
            Some(context_manager),
            None,
        )
    }

    #[tokio::test]
    async fn agent_run_forwards_self_repair_notifications_and_shuts_down_cleanly() {
        let context_manager = Arc::new(ContextManager::new(1));
        let job_id = context_manager
            .create_job("Stuck job", "Needs recovery")
            .await
            .expect("failed to create stuck job");
        context_manager
            .update_context(job_id, |ctx| {
                ctx.transition_to(crate::context::JobState::InProgress, None)
                    .expect("failed to transition job into progress");
                ctx.mark_stuck("simulated stall")
                    .expect("failed to mark job stuck");
            })
            .await
            .expect("failed to update stuck job context");

        let channels = Arc::new(ChannelManager::new());
        let (channel, sender, broadcasts) = BroadcastCaptureChannel::new("test");
        channels.add(Box::new(channel)).await;

        let agent = make_test_agent(
            Arc::clone(&channels),
            Arc::clone(&context_manager),
            Duration::from_millis(10),
            Duration::ZERO,
        );
        let agent_handle = tokio::spawn(agent.run());

        let broadcast = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some((user_id, response)) = broadcasts
                    .lock()
                    .expect("broadcast capture should not be poisoned")
                    .first()
                    .cloned()
                {
                    return (user_id, response);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("self-repair notification should be forwarded");

        assert_eq!(broadcast.0, "default");
        assert!(
            broadcast.1.content.starts_with("Self-Repair: Job "),
            "unexpected notification content: {}",
            broadcast.1.content
        );
        assert!(
            broadcast.1.content.contains("recovery succeeded"),
            "unexpected notification content: {}",
            broadcast.1.content
        );

        sender
            .send(IncomingMessage::new("test", "default", "/quit"))
            .await
            .expect("quit message should send successfully");

        tokio::time::timeout(Duration::from_secs(2), agent_handle)
            .await
            .expect("agent should shut down without deadlocking")
            .expect("agent task should join cleanly")
            .expect("agent run should exit successfully");
    }

    #[test]
    fn test_truncate_short_input() {
        assert_eq!(truncate_for_preview("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_empty_input() {
        assert_eq!(truncate_for_preview("", 10), "");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate_for_preview("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_over_limit() {
        let result = truncate_for_preview("hello world, this is long", 10);
        assert!(result.ends_with("..."));
        // "hello worl" = 10 chars + "..."
        assert_eq!(result, "hello worl...");
    }

    #[test]
    fn test_truncate_collapses_newlines() {
        let result = truncate_for_preview("line1\nline2\nline3", 100);
        assert!(!result.contains('\n'));
        assert_eq!(result, "line1 line2 line3");
    }

    #[test]
    fn test_truncate_collapses_whitespace() {
        let result = truncate_for_preview("hello   world", 100);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_truncate_multibyte_utf8() {
        // Each emoji is 4 bytes. Truncating at char boundary must not panic.
        let input = "😀😁😂🤣😃😄😅😆😉😊";
        let result = truncate_for_preview(input, 5);
        assert!(result.ends_with("..."));
        // First 5 chars = 5 emoji
        assert_eq!(result, "😀😁😂🤣😃...");
    }

    #[test]
    fn test_truncate_cjk_characters() {
        // CJK chars are 3 bytes each in UTF-8.
        let input = "你好世界测试数据很长的字符串";
        let result = truncate_for_preview(input, 4);
        assert_eq!(result, "你好世界...");
    }

    #[test]
    fn test_truncate_mixed_multibyte_and_ascii() {
        let input = "hello 世界 foo";
        let result = truncate_for_preview(input, 8);
        // 'h','e','l','l','o',' ','世','界' = 8 chars
        assert_eq!(result, "hello 世界...");
    }
}
