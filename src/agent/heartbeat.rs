//! Proactive heartbeat system for periodic execution.
//!
//! The heartbeat runner executes periodically (default: every 30 minutes) and:
//! 1. Reads the HEARTBEAT.md checklist
//! 2. Runs an agent turn to process the checklist
//! 3. Reports any findings to the configured channel
//!
//! If nothing needs attention, the agent replies "HEARTBEAT_OK" and no
//! message is sent to the user.
//!
//! # Usage
//!
//! Create a HEARTBEAT.md in the workspace with a checklist of things to monitor:
//!
//! ```markdown
//! # Heartbeat Checklist
//!
//! - [ ] Check for unread emails
//! - [ ] Review calendar for upcoming events
//! - [ ] Check project build status
//! ```
//!
//! The agent will process this checklist on each heartbeat and only notify
//! if action is needed.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::OutgoingResponse;
use crate::db::Database;
use crate::llm::{ChatMessage, CompletionRequest, LlmProvider, Reasoning};
use crate::workspace::Workspace;
use crate::workspace::hygiene::HygieneConfig;

mod checklist;
mod config;

pub use config::HeartbeatConfig;

use checklist::is_effectively_empty;

/// Result of a heartbeat check.
#[derive(Debug)]
pub enum HeartbeatResult {
    /// Nothing needs attention.
    Ok,
    /// Something needs attention, with the message to send.
    NeedsAttention(String),
    /// Heartbeat was skipped (no checklist or disabled).
    Skipped,
    /// Heartbeat failed.
    Failed(String),
}

/// Heartbeat runner for proactive periodic execution.
pub struct HeartbeatRunner {
    config: HeartbeatConfig,
    hygiene_config: HygieneConfig,
    workspace: Arc<Workspace>,
    llm: Arc<dyn LlmProvider>,
    response_tx: Option<mpsc::Sender<OutgoingResponse>>,
    store: Option<Arc<dyn Database>>,
    consecutive_failures: u32,
}

impl HeartbeatRunner {
    /// Create a new heartbeat runner.
    pub fn new(
        config: HeartbeatConfig,
        hygiene_config: HygieneConfig,
        workspace: Arc<Workspace>,
        llm: Arc<dyn LlmProvider>,
    ) -> Self {
        Self {
            config,
            hygiene_config,
            workspace,
            llm,
            response_tx: None,
            store: None,
            consecutive_failures: 0,
        }
    }

    /// Set the response channel for notifications.
    pub fn with_response_channel(mut self, tx: mpsc::Sender<OutgoingResponse>) -> Self {
        self.response_tx = Some(tx);
        self
    }

    /// Set the database store for persistent heartbeat conversations.
    pub fn with_store(mut self, store: Arc<dyn Database>) -> Self {
        self.store = Some(store);
        self
    }

    /// Run the heartbeat loop.
    ///
    /// This runs forever, checking periodically based on the configured interval.
    pub async fn run(&mut self) {
        if !self.config.enabled {
            tracing::info!("Heartbeat is disabled, not starting loop");
            return;
        }

        tracing::info!(
            "Starting heartbeat loop with interval {:?}",
            self.config.interval
        );

        let mut interval = tokio::time::interval(self.config.interval);
        // Don't run immediately on startup
        interval.tick().await;

        loop {
            interval.tick().await;

            // Skip during quiet hours
            if self.config.is_quiet_hours() {
                tracing::trace!("Heartbeat skipped: quiet hours");
                continue;
            }

            self.spawn_hygiene_task();

            let result = self.check_heartbeat().await;
            if !self.handle_result(result).await {
                break;
            }
        }
    }

    /// Run memory hygiene in the background so it never delays the heartbeat
    /// checklist. Failures are logged inside `run_if_due`.
    fn spawn_hygiene_task(&self) {
        let hygiene_workspace = Arc::clone(&self.workspace);
        let hygiene_config = self.hygiene_config.clone();
        tokio::spawn(async move {
            let report =
                crate::workspace::hygiene::run_if_due(&hygiene_workspace, &hygiene_config).await;
            if report.had_work() {
                tracing::info!(
                    daily_logs_deleted = report.daily_logs_deleted,
                    conversation_docs_deleted = report.conversation_docs_deleted,
                    "heartbeat: memory hygiene deleted stale documents"
                );
            }
        });
    }

    /// React to a heartbeat outcome, updating the failure counter and sending
    /// any notification. Returns `false` when the loop should stop.
    async fn handle_result(&mut self, result: HeartbeatResult) -> bool {
        match result {
            HeartbeatResult::Ok => {
                tracing::trace!("Heartbeat OK");
                self.consecutive_failures = 0;
            }
            HeartbeatResult::NeedsAttention(message) => {
                tracing::info!("Heartbeat needs attention: {}", message);
                self.consecutive_failures = 0;
                self.send_notification(&message).await;
            }
            HeartbeatResult::Skipped => {
                tracing::trace!("Heartbeat skipped");
            }
            HeartbeatResult::Failed(error) => {
                tracing::error!("Heartbeat failed: {}", error);
                self.consecutive_failures += 1;

                if self.consecutive_failures >= self.config.max_failures {
                    tracing::error!(
                        "Heartbeat disabled after {} consecutive failures",
                        self.consecutive_failures
                    );
                    return false;
                }
            }
        }
        true
    }

    /// Run a single heartbeat check.
    pub async fn check_heartbeat(&self) -> HeartbeatResult {
        // Get the heartbeat checklist
        let checklist = match self.workspace.heartbeat_checklist().await {
            Ok(Some(content)) if !is_effectively_empty(&content) => content,
            Ok(_) => return HeartbeatResult::Skipped,
            Err(e) => return HeartbeatResult::Failed(format!("Failed to read checklist: {}", e)),
        };

        let messages = self.build_heartbeat_messages(&checklist).await;
        let max_tokens = self.resolve_max_tokens().await;

        let request = CompletionRequest::new(messages)
            .with_max_tokens(max_tokens)
            .with_temperature(0.3);

        let reasoning =
            Reasoning::new(self.llm.clone()).with_model_name(self.llm.active_model_name());
        let (content, _usage) = match reasoning.complete(request).await {
            Ok(r) => r,
            Err(e) => return HeartbeatResult::Failed(format!("LLM call failed: {}", e)),
        };

        let content = content.trim();

        // Guard against empty content. Reasoning models (e.g. GLM-4.7) may
        // burn all output tokens on chain-of-thought and return content: null.
        if content.is_empty() {
            return HeartbeatResult::Failed("LLM returned empty content.".to_string());
        }

        // Check if nothing needs attention
        if content == "HEARTBEAT_OK" || content.contains("HEARTBEAT_OK") {
            return HeartbeatResult::Ok;
        }

        HeartbeatResult::NeedsAttention(content.to_string())
    }

    /// Build the chat messages for a heartbeat turn: the checklist prompt,
    /// preceded by the workspace system prompt when one is available.
    async fn build_heartbeat_messages(&self, checklist: &str) -> Vec<ChatMessage> {
        let prompt = format!(
            "Read the HEARTBEAT.md checklist below and follow it strictly. \
             Do not infer or repeat old tasks. Check each item and report findings.\n\
             \n\
             If nothing needs attention, reply EXACTLY with: HEARTBEAT_OK\n\
             \n\
             If something needs attention, provide a concise summary of what needs action.\n\
             \n\
             ## HEARTBEAT.md\n\
             \n\
             {}",
            checklist
        );

        let system_prompt = match self.workspace.system_prompt().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to get system prompt for heartbeat: {}", e);
                String::new()
            }
        };

        if system_prompt.is_empty() {
            vec![ChatMessage::user(&prompt)]
        } else {
            vec![
                ChatMessage::system(&system_prompt),
                ChatMessage::user(&prompt),
            ]
        }
    }

    /// Derive `max_tokens` from the model's context length: half the context
    /// window (the rest is the prompt), with a floor of 4096.
    async fn resolve_max_tokens(&self) -> u32 {
        match self.llm.model_metadata().await {
            Ok(meta) => {
                let from_api = meta.context_length.map(|ctx| ctx / 2).unwrap_or(4096);
                from_api.max(4096)
            }
            Err(e) => {
                tracing::warn!(
                    "Could not fetch model metadata, using default max_tokens: {}",
                    e
                );
                4096
            }
        }
    }

    /// Send a notification about heartbeat findings.
    async fn send_notification(&self, message: &str) {
        let Some(ref tx) = self.response_tx else {
            tracing::debug!("No response channel configured for heartbeat notifications");
            return;
        };

        let user_id = self.config.notify_user_id.as_deref().unwrap_or("default");
        let thread_id = self.persist_heartbeat_message(user_id, message).await;

        let response = OutgoingResponse {
            content: format!("🔔 *Heartbeat Alert*\n\n{}", message),
            thread_id,
            attachments: Vec::new(),
            metadata: serde_json::json!({
                "source": "heartbeat",
            }),
        };

        if let Err(e) = tx.send(response).await {
            tracing::error!("Failed to send heartbeat notification: {}", e);
        }
    }

    /// Persist the message to the heartbeat conversation, returning its
    /// thread identifier when a store is configured and the write succeeds.
    async fn persist_heartbeat_message(&self, user_id: &str, message: &str) -> Option<String> {
        let store = self.store.as_ref()?;
        match store.get_or_create_heartbeat_conversation(user_id).await {
            Ok(conv_id) => {
                if let Err(e) = store
                    .add_conversation_message(conv_id, "assistant", message)
                    .await
                {
                    tracing::error!("Failed to persist heartbeat message: {}", e);
                }
                Some(conv_id.to_string())
            }
            Err(e) => {
                tracing::error!("Failed to get heartbeat conversation: {}", e);
                None
            }
        }
    }
}

/// Spawn the heartbeat runner as a background task.
///
/// Returns a handle that can be used to stop the runner.
pub fn spawn_heartbeat(
    config: HeartbeatConfig,
    hygiene_config: HygieneConfig,
    workspace: Arc<Workspace>,
    llm: Arc<dyn LlmProvider>,
    response_tx: Option<mpsc::Sender<OutgoingResponse>>,
    store: Option<Arc<dyn Database>>,
) -> tokio::task::JoinHandle<()> {
    let mut runner = HeartbeatRunner::new(config, hygiene_config, workspace, llm);
    if let Some(tx) = response_tx {
        runner = runner.with_response_channel(tx);
    }
    if let Some(s) = store {
        runner = runner.with_store(s);
    }

    tokio::spawn(async move {
        runner.run().await;
    })
}

#[cfg(test)]
mod tests;
