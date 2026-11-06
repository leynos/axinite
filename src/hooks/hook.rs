//! Core hook types and traits.

use std::time::Duration;

use core::future::Future;
use core::pin::Pin;

use serde::{Deserialize, Serialize};

/// Points in the agent lifecycle where hooks can be attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HookPoint {
    /// Before processing an inbound user message.
    BeforeInbound,
    /// Before executing a tool call.
    BeforeToolCall,
    /// Before sending an outbound response.
    BeforeOutbound,
    /// When a new session starts.
    OnSessionStart,
    /// When a session ends (pruned or expired).
    OnSessionEnd,
    /// Transform the final response before completing a turn.
    TransformResponse,
}

impl HookPoint {
    /// Human-readable hook point identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            HookPoint::BeforeInbound => "beforeInbound",
            HookPoint::BeforeToolCall => "beforeToolCall",
            HookPoint::BeforeOutbound => "beforeOutbound",
            HookPoint::OnSessionStart => "onSessionStart",
            HookPoint::OnSessionEnd => "onSessionEnd",
            HookPoint::TransformResponse => "transformResponse",
        }
    }
}

/// Contextual data carried with each hook invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookEvent {
    /// An inbound user message about to be processed.
    Inbound {
        /// ID of the user who sent the message.
        user_id: String,
        /// Name of the channel the message arrived on.
        channel: String,
        /// Message text. A `BeforeInbound` hook may replace it with a modification.
        content: String,
        /// Thread the message belongs to, when the channel supplies one.
        thread_id: Option<String>,
    },
    /// A tool call about to be executed.
    ToolCall {
        /// Name of the tool being called.
        tool_name: String,
        /// Tool arguments as JSON. A `BeforeToolCall` hook may replace them with a JSON
        /// modification.
        parameters: serde_json::Value,
        /// ID of the user on whose behalf the tool is called.
        user_id: String,
        /// "chat" for interactive, or a job ID string for autonomous jobs.
        context: String,
    },
    /// An outbound response about to be sent.
    Outbound {
        /// ID of the user the response is addressed to.
        user_id: String,
        /// Name of the channel the response will be delivered on.
        channel: String,
        /// Response text. A `BeforeOutbound` hook may replace it with a modification.
        content: String,
        /// Thread the response belongs to, when there is one.
        thread_id: Option<String>,
    },
    /// A new session was created.
    SessionStart {
        /// Owner of the new session.
        user_id: String,
        /// Identifier of the new session.
        session_id: String,
    },
    /// A session was ended (pruned).
    SessionEnd {
        /// Owner of the session that ended.
        user_id: String,
        /// Identifier of the session that ended.
        session_id: String,
    },
    /// The final response is being transformed before completing a turn.
    ResponseTransform {
        /// Owner of the thread.
        user_id: String,
        /// Thread the response completes a turn in.
        thread_id: String,
        /// Final response text. A `TransformResponse` hook may replace it.
        response: String,
    },
}

impl HookEvent {
    /// Returns the [`HookPoint`] this event corresponds to.
    pub fn hook_point(&self) -> HookPoint {
        match self {
            HookEvent::Inbound { .. } => HookPoint::BeforeInbound,
            HookEvent::ToolCall { .. } => HookPoint::BeforeToolCall,
            HookEvent::Outbound { .. } => HookPoint::BeforeOutbound,
            HookEvent::SessionStart { .. } => HookPoint::OnSessionStart,
            HookEvent::SessionEnd { .. } => HookPoint::OnSessionEnd,
            HookEvent::ResponseTransform { .. } => HookPoint::TransformResponse,
        }
    }

    /// Apply a modification string to the event's primary content field.
    pub fn apply_modification(&mut self, modified: &str) {
        match self {
            HookEvent::Inbound { content, .. } | HookEvent::Outbound { content, .. } => {
                *content = modified.to_string();
            }
            HookEvent::ToolCall { parameters, .. } => match serde_json::from_str(modified) {
                Ok(parsed) => *parameters = parsed,
                Err(e) => {
                    tracing::warn!(
                        "Hook returned non-JSON modification for ToolCall, ignoring: {}",
                        e
                    );
                }
            },
            HookEvent::ResponseTransform { response, .. } => {
                *response = modified.to_string();
            }
            HookEvent::SessionStart { .. } | HookEvent::SessionEnd { .. } => {
                // Session events don't have modifiable content
            }
        }
    }
}

/// The result of executing a hook.
#[derive(Debug, Clone)]
pub enum HookOutcome {
    /// Continue processing, optionally with modified content.
    Continue {
        /// If `Some`, replace the event's primary content with this value.
        modified: Option<String>,
    },
    /// Reject the event entirely.
    Reject {
        /// Human-readable reason for the rejection.
        reason: String,
    },
}

impl HookOutcome {
    /// Shorthand for `Continue { modified: None }`.
    pub fn ok() -> Self {
        HookOutcome::Continue { modified: None }
    }

    /// Shorthand for `Continue { modified: Some(value) }`.
    pub fn modify(value: String) -> Self {
        HookOutcome::Continue {
            modified: Some(value),
        }
    }

    /// Shorthand for `Reject { reason }`.
    pub fn reject(reason: impl Into<String>) -> Self {
        HookOutcome::Reject {
            reason: reason.into(),
        }
    }
}

/// How to handle hook execution failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFailureMode {
    /// On error/timeout, continue processing as if the hook returned `ok()`.
    FailOpen,
    /// On error/timeout, reject the event.
    FailClosed,
}

/// Hook execution errors.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    /// The hook failed. The registry returns this only for a `FailClosed` hook; a
    /// `FailOpen` hook's failure is logged and the chain continues.
    #[error("Hook execution failed: {reason}")]
    ExecutionFailed {
        /// Description of the failure, prefixed with the hook's name by the registry.
        reason: String,
    },

    /// The hook exceeded its `timeout()`. As with `ExecutionFailed`, the registry returns
    /// this only for a `FailClosed` hook.
    #[error("Hook timed out after {timeout:?}")]
    Timeout {
        /// The limit that was exceeded.
        timeout: Duration,
    },

    /// The hook returned `HookOutcome::Reject`, which stops the chain at that hook. This
    /// is a deliberate veto, not a failure, so the failure mode does not apply.
    #[error("Hook rejected: {reason}")]
    Rejected {
        /// Reason the hook gave for the rejection.
        reason: String,
    },
}

/// Context passed to hooks alongside the event.
pub struct HookContext {
    /// Arbitrary metadata hooks can use.
    pub metadata: serde_json::Value,
}

impl Default for HookContext {
    fn default() -> Self {
        Self {
            metadata: serde_json::Value::Null,
        }
    }
}

/// Boxed future used at the dyn hook boundary.
pub type HookFuture<'a> = Pin<Box<dyn Future<Output = Result<HookOutcome, HookError>> + Send + 'a>>;

/// Trait for implementing lifecycle hooks.
///
/// Hooks intercept and can modify agent operations at well-defined points.
pub trait Hook: Send + Sync {
    /// A unique name for this hook.
    fn name(&self) -> &str;

    /// The lifecycle points this hook should be called at.
    fn hook_points(&self) -> &[HookPoint];

    /// How to handle failures in this hook.
    ///
    /// Default: `FailOpen` (continue on error).
    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailOpen
    }

    /// Maximum time this hook is allowed to run.
    ///
    /// Default: 5 seconds.
    fn timeout(&self) -> Duration {
        Duration::from_secs(5)
    }

    /// Execute the hook.
    fn execute<'a>(&'a self, event: &'a HookEvent, ctx: &'a HookContext) -> HookFuture<'a>;
}

/// Native async sibling trait for concrete hook implementations.
///
/// Implement this trait on concrete hook types. The blanket adapter
/// automatically satisfies [`Hook`] (including the sync methods) so
/// that they can be used as `Arc<dyn Hook>` or `Box<dyn Hook>`.
///
/// The sync methods (`name`, `hook_points`, `failure_mode`, `timeout`)
/// must also be implemented here because the blanket impl for `Hook`
/// delegates to them.
pub trait NativeHook: Send + Sync {
    /// See [`Hook::name`].
    fn name(&self) -> &str;

    /// See [`Hook::hook_points`].
    fn hook_points(&self) -> &[HookPoint];

    /// See [`Hook::failure_mode`].
    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailOpen
    }

    /// See [`Hook::timeout`].
    fn timeout(&self) -> Duration {
        Duration::from_secs(5)
    }

    /// See [`Hook::execute`].
    fn execute<'a>(
        &'a self,
        event: &'a HookEvent,
        ctx: &'a HookContext,
    ) -> impl Future<Output = Result<HookOutcome, HookError>> + Send + 'a;
}

impl<T> Hook for T
where
    T: NativeHook + Send + Sync,
{
    fn name(&self) -> &str {
        NativeHook::name(self)
    }

    fn hook_points(&self) -> &[HookPoint] {
        NativeHook::hook_points(self)
    }

    fn failure_mode(&self) -> HookFailureMode {
        NativeHook::failure_mode(self)
    }

    fn timeout(&self) -> Duration {
        NativeHook::timeout(self)
    }

    fn execute<'a>(&'a self, event: &'a HookEvent, ctx: &'a HookContext) -> HookFuture<'a> {
        Box::pin(NativeHook::execute(self, event, ctx))
    }
}
