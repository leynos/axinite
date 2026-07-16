//! Auth-intercept handling for tool execution: entering auth mode,
//! extension activation, and the `process_auth_token` entry point.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::dispatcher::parse_auth_result;
use crate::agent::session::{PendingApproval, Session};
use crate::agent::thread_ops::TurnPersistContext;
use crate::channels::StatusUpdate;
use crate::error::Error;

use super::context::{MsgEnv, TurnScope};

/// Parameters for auth intercept handling.
pub(super) struct AuthInterceptParams<'a> {
    /// Session containing the thread.
    pub(super) session: &'a Arc<Mutex<Session>>,
    /// Thread ID for the conversation.
    pub(super) thread_id: Uuid,
    /// Message environment context.
    pub(super) env: &'a MsgEnv,
    /// Tool execution result (used to extract auth URLs).
    pub(super) tool_result: &'a Result<String, Error>,
    /// Extension name requiring authentication.
    pub(super) ext_name: String,
    /// Instructions to display to the user.
    pub(super) instructions: String,
    /// Pending approval to preserve for continuation after auth.
    pub(super) pending: Option<PendingApproval>,
}

/// Parameters for auth re-entry.
struct AuthReentry {
    ext_name: String,
    instructions: String,
    auth_url: Option<String>,
    setup_url: Option<String>,
}

impl Agent {
    /// Handle an auth-required result from a tool execution.
    ///
    /// Enters auth mode on the thread, stores the pending approval (if provided)
    /// to preserve deferred tool calls and context messages, completes + persists
    /// the turn, and sends the AuthRequired status to the channel.
    pub(super) async fn handle_auth_intercept(&self, params: AuthInterceptParams<'_>) {
        let auth_data = parse_auth_result(params.tool_result);
        let (turn_number, tool_calls) = {
            let mut sess = params.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&params.thread_id) {
                // Complete turn first (resets state to Idle)
                thread.complete_turn(&params.instructions);
                // Store pending approval to preserve deferred tool calls and context
                // messages so the tool chain can resume after auth completion.
                if let Some(pending) = params.pending {
                    thread.await_approval(pending);
                }
                // Set pending auth (state unchanged)
                thread.enter_auth_mode(params.ext_name.clone());
                thread
                    .turns
                    .last()
                    .map(|turn| (turn.turn_number, turn.tool_calls.clone()))
                    .unwrap_or((0, Vec::new()))
            } else {
                (0, Vec::new())
            }
        };

        if turn_number != 0 {
            let persist_ctx = TurnPersistContext {
                thread_id: params.thread_id,
                user_id: &params.env.user_id,
                turn_number,
            };
            self.persist_tool_calls(&persist_ctx, &tool_calls).await;
        }

        // User message already persisted at turn start; save auth instructions
        self.persist_assistant_response(
            params.thread_id,
            &params.env.user_id,
            &params.instructions,
        )
        .await;
        let _ = self
            .channels
            .send_status(
                &params.env.channel,
                StatusUpdate::AuthRequired {
                    extension_name: params.ext_name,
                    instructions: Some(params.instructions.clone()),
                    auth_url: auth_data.auth_url,
                    setup_url: auth_data.setup_url,
                },
                &params.env.metadata,
            )
            .await;
    }

    /// Activate extension after successful auth and notify.
    async fn activate_extension_and_notify(&self, env: &MsgEnv, ext_name: &str) -> Option<String> {
        let ext_mgr = match self.deps.extension_manager.as_ref() {
            Some(mgr) => mgr,
            None => {
                return Some(format!(
                    "{} authenticated, but extension manager is unavailable.",
                    ext_name
                ));
            }
        };

        match ext_mgr.activate(ext_name).await {
            Ok(activate_result) => {
                let tool_count = activate_result.tools_loaded.len();
                let tool_list = if activate_result.tools_loaded.is_empty() {
                    String::new()
                } else {
                    format!("\n\nTools: {}", activate_result.tools_loaded.join(", "))
                };
                let msg = format!(
                    "{} authenticated and activated ({} tools loaded).{}",
                    ext_name, tool_count, tool_list
                );
                let _ = self
                    .channels
                    .send_status(
                        &env.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: ext_name.to_string(),
                            success: true,
                            message: msg.clone(),
                        },
                        &env.metadata,
                    )
                    .await;
                Some(msg)
            }
            Err(e) => {
                tracing::warn!(
                    "Extension '{}' authenticated but activation failed: {}",
                    ext_name,
                    e
                );
                let msg = format!(
                    "{} authenticated successfully, but activation failed: {}. \
                     Try activating manually.",
                    ext_name, e
                );
                let _ = self
                    .channels
                    .send_status(
                        &env.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: ext_name.to_string(),
                            success: false,
                            message: msg.clone(),
                        },
                        &env.metadata,
                    )
                    .await;
                Some(msg)
            }
        }
    }

    /// Re-enter auth mode and notify.
    async fn reenter_auth_mode_and_notify(
        &self,
        scope: &TurnScope,
        reentry: AuthReentry,
    ) -> Option<String> {
        {
            let mut sess = scope.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                thread.enter_auth_mode(reentry.ext_name.clone());
            }
        }
        let _ = self
            .channels
            .send_status(
                &scope.env.channel,
                StatusUpdate::AuthRequired {
                    extension_name: reentry.ext_name.clone(),
                    instructions: Some(reentry.instructions.clone()),
                    auth_url: reentry.auth_url,
                    setup_url: reentry.setup_url,
                },
                &scope.env.metadata,
            )
            .await;
        Some(reentry.instructions)
    }

    /// Handle an auth token submitted while the thread is in auth mode.
    ///
    /// The token goes directly to the extension manager's credential store,
    /// completely bypassing logging, turn creation, history, and compaction.
    pub(in crate::agent::thread_ops) async fn process_auth_token(
        &self,
        scope: TurnScope,
        pending: &crate::agent::session::PendingAuth,
        token: &str,
    ) -> Result<Option<String>, Error> {
        let token = token.trim();

        let ext_mgr = match self.deps.extension_manager.as_ref() {
            Some(mgr) => mgr,
            None => return Ok(Some("Extension manager not available.".to_string())),
        };

        match ext_mgr.auth(&pending.extension_name, Some(token)).await {
            Ok(result) if result.is_authenticated() => {
                {
                    let mut sess = scope.session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                        thread.pending_auth = None;
                        thread.clear_pending_approval();
                    }
                }
                tracing::info!(
                    "Extension '{}' authenticated via auth mode",
                    pending.extension_name
                );

                // Auto-activate so tools are available immediately after auth
                Ok(self
                    .activate_extension_and_notify(&scope.env, &pending.extension_name)
                    .await)
            }
            Ok(result) => {
                // Invalid token, re-enter auth mode
                let instructions = result
                    .instructions()
                    .map(String::from)
                    .unwrap_or_else(|| "Invalid token. Please try again.".to_string());
                let auth_url = result.auth_url().map(String::from);
                let setup_url = result.setup_url().map(String::from);
                let reentry = AuthReentry {
                    ext_name: pending.extension_name.clone(),
                    instructions,
                    auth_url,
                    setup_url,
                };
                let _ = self.reenter_auth_mode_and_notify(&scope, reentry).await;
                Ok(None)
            }
            Err(e) => {
                let msg = format!(
                    "Authentication failed for {}: {}",
                    pending.extension_name, e
                );
                // Restore pending_auth so the next user message is still intercepted
                {
                    let mut sess = scope.session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                        thread.pending_auth = Some(pending.clone());
                    }
                }
                // Re-enter auth mode to allow retry
                let reentry = AuthReentry {
                    ext_name: pending.extension_name.clone(),
                    instructions: format!("{} Please try again.", msg),
                    auth_url: None,
                    setup_url: None,
                };
                let _ = self.reenter_auth_mode_and_notify(&scope, reentry).await;
                Ok(None)
            }
        }
    }
}
