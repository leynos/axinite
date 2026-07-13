//! Thread model: conversation threads within a session, including thread
//! state, pending approval/auth requests, and checkpoint restoration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::channels::web::util::truncate_preview;
use crate::llm::{ChatMessage, ToolCall};

use super::restore::{consume_final_response, consume_tool_call_rounds};
use super::turn::Turn;

/// State of a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadState {
    /// Thread is idle, waiting for input.
    Idle,
    /// Thread is processing a turn.
    Processing,
    /// Thread is waiting for user approval.
    AwaitingApproval,
    /// Thread has completed (no more turns expected).
    Completed,
    /// Thread was interrupted.
    Interrupted,
}

/// Pending auth token request.
///
/// When `tool_auth` returns `awaiting_token`, the thread enters auth mode.
/// The next user message is intercepted before entering the normal pipeline
/// (no logging, no turn creation, no history) and routed directly to the
/// credential store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAuth {
    /// Extension name to authenticate.
    pub extension_name: String,
}

/// Pending tool approval request stored on a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    /// Unique request ID.
    pub request_id: Uuid,
    /// Tool name requiring approval.
    pub tool_name: String,
    /// Tool parameters (original values, used for execution).
    pub parameters: serde_json::Value,
    /// Redacted tool parameters (sensitive values replaced with `[REDACTED]`).
    /// Used for display in approval UI, logs, and SSE broadcasts.
    #[serde(default)]
    pub display_parameters: serde_json::Value,
    /// Description of what the tool will do.
    pub description: String,
    /// Tool call ID from LLM (for proper context continuation).
    pub tool_call_id: String,
    /// Context messages at the time of the request (to resume from).
    pub context_messages: Vec<ChatMessage>,
    /// Remaining tool calls from the same assistant message that were not
    /// executed yet when approval was requested.
    #[serde(default)]
    pub deferred_tool_calls: Vec<ToolCall>,
    /// User timezone at the time the approval was requested, so it persists
    /// through the approval flow even if the approval message lacks timezone.
    #[serde(default)]
    pub user_timezone: Option<String>,
}

/// A conversation thread within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Unique thread ID.
    pub id: Uuid,
    /// Parent session ID.
    pub session_id: Uuid,
    /// Current state.
    pub state: ThreadState,
    /// Turns in this thread.
    pub turns: Vec<Turn>,
    /// When the thread was created.
    pub created_at: DateTime<Utc>,
    /// When the thread was last updated.
    pub updated_at: DateTime<Utc>,
    /// Thread metadata (e.g., title, tags).
    pub metadata: serde_json::Value,
    /// Pending approval request (when state is AwaitingApproval).
    #[serde(default)]
    pub pending_approval: Option<PendingApproval>,
    /// Pending auth token request (thread is in auth mode).
    #[serde(default)]
    pub pending_auth: Option<PendingAuth>,
    /// In-flight auth marker to prevent concurrent auth bypass.
    /// Transient: not serialized, always starts as false on deserialization.
    #[serde(skip)]
    pub in_flight_auth: bool,
}

impl Thread {
    fn init(session_id: Uuid, thread_id: Uuid) -> Self {
        let now = Utc::now();
        Self {
            id: thread_id,
            session_id,
            state: ThreadState::Idle,
            turns: Vec::new(),
            created_at: now,
            updated_at: now,
            metadata: serde_json::Value::Null,
            pending_approval: None,
            pending_auth: None,
            in_flight_auth: false,
        }
    }

    /// Create a new thread.
    pub fn new(session_id: Uuid) -> Self {
        let thread_id = Uuid::new_v4();
        Self::init(session_id, thread_id)
    }

    /// Create a thread with a specific ID (for DB hydration).
    pub fn with_id(id: Uuid, session_id: Uuid) -> Self {
        Self::init(session_id, id)
    }

    /// Get the current turn number (1-indexed for display).
    pub fn turn_number(&self) -> usize {
        self.turns.len() + 1
    }

    /// Get the last turn.
    pub fn last_turn(&self) -> Option<&Turn> {
        self.turns.last()
    }

    /// Get the last turn mutably.
    pub fn last_turn_mut(&mut self) -> Option<&mut Turn> {
        self.turns.last_mut()
    }

    /// Start a new turn with user input.
    pub fn start_turn(&mut self, user_input: impl Into<String>) -> &mut Turn {
        let turn_number = self.turns.len();
        let turn = Turn::new(turn_number, user_input);
        self.turns.push(turn);
        self.state = ThreadState::Processing;
        self.updated_at = Utc::now();
        // turn_number was len() before push, so it's a valid index after push
        &mut self.turns[turn_number]
    }

    /// Complete the current turn with a response.
    pub fn complete_turn(&mut self, response: impl Into<String>) {
        if let Some(turn) = self.turns.last_mut() {
            turn.complete(response);
        }
        self.state = ThreadState::Idle;
        self.updated_at = Utc::now();
    }

    /// Fail the current turn with an error.
    pub fn fail_turn(&mut self, error: impl Into<String>) {
        if let Some(turn) = self.turns.last_mut() {
            turn.fail(error);
        }
        self.state = ThreadState::Idle;
        self.updated_at = Utc::now();
    }

    /// Mark the thread as awaiting approval with pending request details.
    pub fn await_approval(&mut self, pending: PendingApproval) {
        self.state = ThreadState::AwaitingApproval;
        self.pending_approval = Some(pending);
        self.updated_at = Utc::now();
    }

    /// Take the pending approval (clearing it from the thread).
    pub fn take_pending_approval(&mut self) -> Option<PendingApproval> {
        self.pending_approval.take()
    }

    /// Clear pending approval and return to idle state.
    pub fn clear_pending_approval(&mut self) {
        self.pending_approval = None;
        self.state = ThreadState::Idle;
        self.updated_at = Utc::now();
    }

    /// Enter auth mode: next user message will be routed directly to
    /// the credential store, bypassing the normal pipeline entirely.
    pub fn enter_auth_mode(&mut self, extension_name: String) {
        self.pending_auth = Some(PendingAuth { extension_name });
        self.updated_at = Utc::now();
    }

    /// Take the pending auth (clearing auth mode).
    pub fn take_pending_auth(&mut self) -> Option<PendingAuth> {
        self.pending_auth.take()
    }

    /// Interrupt the current turn.
    pub fn interrupt(&mut self) {
        if let Some(turn) = self.turns.last_mut() {
            turn.interrupt();
        }
        self.state = ThreadState::Interrupted;
        self.updated_at = Utc::now();
    }

    /// Resume after interruption.
    pub fn resume(&mut self) {
        if self.state == ThreadState::Interrupted {
            self.state = ThreadState::Idle;
            self.updated_at = Utc::now();
        }
    }

    /// Get all messages for context building, including tool call history.
    ///
    /// Emits the full LLM-compatible message sequence per turn:
    /// `user → [assistant_with_tool_calls → tool_result*] → assistant`
    ///
    /// This ensures the LLM sees prior tool executions and won't re-attempt
    /// completed actions in subsequent turns.
    pub fn messages(&self) -> Vec<ChatMessage> {
        let mut messages = Vec::new();
        for turn in &self.turns {
            if turn.image_content_parts.is_empty() {
                messages.push(ChatMessage::user(&turn.user_input));
            } else {
                messages.push(ChatMessage::user_with_parts(
                    &turn.user_input,
                    turn.image_content_parts.clone(),
                ));
            }

            if !turn.tool_calls.is_empty() {
                // Build ToolCall objects with synthetic stable IDs
                let tool_calls: Vec<ToolCall> = turn
                    .tool_calls
                    .iter()
                    .enumerate()
                    .map(|(i, tc)| ToolCall {
                        id: format!("turn{}_{}", turn.turn_number, i),
                        name: tc.name.clone(),
                        arguments: tc.parameters.clone(),
                    })
                    .collect();

                // Assistant message declaring the tool calls (no text content)
                messages.push(ChatMessage::assistant_with_tool_calls(None, tool_calls));

                // Individual tool result messages, truncated to limit context size.
                for (i, tc) in turn.tool_calls.iter().enumerate() {
                    let call_id = format!("turn{}_{}", turn.turn_number, i);
                    let content = if let Some(ref err) = tc.error {
                        // .error already contains the full error text;
                        // pass through without wrapping to avoid double-prefix.
                        truncate_preview(err, 1000)
                    } else if let Some(ref res) = tc.result {
                        let raw = match res {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        truncate_preview(&raw, 1000)
                    } else {
                        "OK".to_string()
                    };
                    messages.push(ChatMessage::tool_result(call_id, &tc.name, content));
                }
            }
            if let Some(ref response) = turn.response {
                messages.push(ChatMessage::assistant(response));
            }
        }
        messages
    }

    /// Truncate turns to a specific count (keeping most recent).
    pub fn truncate_turns(&mut self, keep: usize) {
        if self.turns.len() > keep {
            let drain_count = self.turns.len() - keep;
            self.turns.drain(0..drain_count);
            // Re-number remaining turns
            for (i, turn) in self.turns.iter_mut().enumerate() {
                turn.turn_number = i;
            }
        }
    }

    /// Restore thread state from a checkpoint's messages.
    ///
    /// Clears existing turns and rebuilds from the message sequence.
    /// Handles the full message pattern including tool messages:
    /// `user → [assistant_with_tool_calls → tool_result*] → assistant`
    ///
    /// Also supports the legacy pattern (user/assistant pairs only) for
    /// backward compatibility with old checkpoint data.
    pub fn restore_from_messages(&mut self, messages: Vec<ChatMessage>) {
        self.turns.clear();
        self.state = ThreadState::Idle;

        let mut iter = messages.into_iter().peekable();
        let mut turn_number = 0;

        while let Some(msg) = iter.next() {
            // Skip non-user messages that aren't anchored to a turn
            if msg.role != crate::llm::Role::User {
                continue;
            }
            let mut turn = Turn::new(turn_number, &msg.content);
            consume_tool_call_rounds(&mut iter, &mut turn);
            consume_final_response(&mut iter, &mut turn);
            self.turns.push(turn);
            turn_number += 1;
        }

        self.updated_at = Utc::now();
    }
}
