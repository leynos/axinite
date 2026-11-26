//! Parent-side mediation of guest tool callbacks.
//!
//! A [`BrokerContext`] is the immutable authority a caller issues for one
//! `exec_code` invocation: the selected tools, their pinned identities, the
//! caller's current auto-approval snapshot, and the safety and hook services
//! the callback must pass through. A callback that fails any check returns a
//! typed [`BrokerDenial`] and performs no side effect.

use std::collections::HashSet;
use std::sync::Arc;

use monty_protocol::{DenialCode, MAX_FRAME_BYTES, validate_arguments};
use serde_json::Value;

use crate::context::JobContext;
use crate::hooks::{HookEvent, HookOutcome};
use crate::safety::SafetyLayer;
use crate::tools::monty::capabilities::{SelectedTools, SelectionError};
use crate::tools::{
    ApprovalRequirement, Tool, ToolRegistry, execute::execute_resolved_tool_with_safety,
    redact_params,
};

/// The largest tool result the broker will hand to the helper.
///
/// The frame ceiling covers the whole envelope, so a result that alone filled
/// it could not be encoded with its run id, call id and framing.
pub const MAX_BROKER_RESULT_BYTES: usize = MAX_FRAME_BYTES - 16 * 1024;

/// The caller's current policy snapshot for nested callbacks.
#[derive(Clone, Debug, Default)]
pub struct CallerPolicy {
    /// The user the caller acts for.
    pub user_id: String,
    /// The caller's hook context label.
    pub hook_context: String,
    /// Tools the caller's session has already auto-approved.
    pub auto_approved: HashSet<String>,
    /// The authority ceiling the caller advertises: the names its own skill
    /// attenuation left visible.
    ///
    /// `None` means the caller supplied no snapshot at all — a legacy pending
    /// request, say — so the registry, hook and approval checks are the only
    /// ceiling. `Some` is always authoritative, including when it is empty: a
    /// caller whose attenuation left nothing visible must not be able to name
    /// a registered tool. An empty set and "no ceiling" are therefore
    /// different values, and collapsing them would let a caller with no
    /// visible tools select any registered one.
    pub permitted: Option<HashSet<String>>,
}

impl CallerPolicy {
    /// Report whether the caller's own ceiling admits `name`.
    pub fn permits(&self, name: &str) -> bool {
        self.permitted
            .as_ref()
            .is_none_or(|permitted| permitted.contains(name))
    }
}

/// The immutable services one `exec_code` invocation's broker needs.
///
/// Grouping them keeps every `BrokerContext` method to the callback it
/// mediates, rather than repeating the registry, safety layer and hooks.
#[derive(Clone)]
pub struct BrokerServices {
    /// The registry callbacks resolve against.
    pub tools: Arc<ToolRegistry>,
    /// The layer every parameter and result passes through.
    pub safety: Arc<SafetyLayer>,
    /// The caller's hook registry, when the caller has one.
    pub hooks: Option<Arc<crate::hooks::HookRegistry>>,
    /// The caller's policy snapshot.
    pub policy: CallerPolicy,
}

/// One callback being mediated: its name, index in the table, arguments and
/// the job it runs for.
struct Callback<'a> {
    name: &'a str,
    index: usize,
    arguments: &'a Value,
    job_ctx: &'a JobContext,
}

/// A typed refusal to service a callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerDenial {
    /// The wire failure class.
    pub code: DenialCode,
    /// A redacted, bounded operator-facing message.
    pub message: String,
}

impl BrokerDenial {
    /// Creates a denial with the given wire code.
    pub fn new(code: DenialCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The denial produced when a caller supplies no broker capability.
    pub fn unavailable() -> Self {
        Self::new(
            DenialCode::BrokerUnavailable,
            "exec_code requires a caller-issued broker",
        )
    }
}

impl std::fmt::Display for BrokerDenial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.message)
    }
}

/// The authority carried by a caller into one `exec_code` invocation.
pub struct BrokerContext {
    table: SelectedTools,
    services: BrokerServices,
}

impl std::fmt::Debug for BrokerContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BrokerContext")
            .field("table", &self.table)
            .field("user_id", &self.services.policy.user_id)
            .field("hooks", &self.services.hooks.is_some())
            .finish_non_exhaustive()
    }
}

impl BrokerContext {
    /// Builds the authority for one run.
    ///
    /// The requested names are checked against the caller's own ceiling before
    /// the registry is consulted, so a run cannot name a tool the caller's
    /// skill attenuation removed.
    ///
    /// # Errors
    ///
    /// Returns [`SelectionError`] when the requested names cannot be resolved
    /// into a valid immutable table, or when one lies outside the ceiling.
    pub async fn new(
        services: BrokerServices,
        requested: &[String],
    ) -> Result<Self, SelectionError> {
        if let Some(name) = requested.iter().find(|name| !services.policy.permits(name)) {
            return Err(SelectionError::OutsideCallerAuthority { name: name.clone() });
        }
        let table = SelectedTools::resolve(&services.tools, requested).await?;
        Ok(Self { table, services })
    }

    /// The selected table, for the start message.
    pub fn table(&self) -> &SelectedTools {
        &self.table
    }

    /// Answers one callback on behalf of `job_ctx`.
    ///
    /// Membership, pinned identity, schema and approval are checked before
    /// dispatch. A hook may rewrite the arguments; when it does, every check
    /// is repeated against the rewritten call before anything executes.
    ///
    /// # Errors
    ///
    /// Returns a [`BrokerDenial`] when the call is refused. No refusal
    /// dispatches a tool.
    pub async fn mediate(
        &self,
        name: &str,
        arguments: &Value,
        job_ctx: &JobContext,
    ) -> Result<Value, BrokerDenial> {
        if name == "exec_code" {
            return Err(BrokerDenial::new(
                DenialCode::RecursionDenied,
                "exec_code cannot be called from guest code",
            ));
        }
        let index = self
            .table
            .index_of(name)
            .ok_or_else(|| BrokerDenial::new(DenialCode::NotAllowed, "tool is not selected"))?;
        if !self.table.admits(index) {
            return Err(BrokerDenial::new(
                DenialCode::NotAllowed,
                "tool is outside the run's capability view",
            ));
        }
        if !arguments.is_object() {
            return Err(BrokerDenial::new(
                DenialCode::ContractError,
                "tool arguments must be a JSON object",
            ));
        }

        let arguments = self.run_hooks(name, arguments).await?;
        let callback = Callback {
            name,
            index,
            arguments: &arguments,
            job_ctx,
        };
        self.check_and_dispatch(&callback).await
    }

    async fn check_and_dispatch(&self, callback: &Callback<'_>) -> Result<Value, BrokerDenial> {
        let Callback {
            name,
            index,
            arguments,
            ..
        } = *callback;
        let entry = self
            .table
            .entries()
            .get(index)
            .ok_or_else(|| BrokerDenial::new(DenialCode::NotAllowed, "tool is not selected"))?;

        // A tool replaced after selection is not the tool the caller
        // authorized. Refuse rather than silently run the new implementation.
        let current = self.services.tools.get(name).await.ok_or_else(|| {
            BrokerDenial::new(DenialCode::PolicyDenied, "tool is no longer registered")
        })?;
        if !Arc::ptr_eq(entry.tool(), &current) {
            return Err(BrokerDenial::new(
                DenialCode::PolicyDenied,
                "tool identity changed after selection",
            ));
        }

        if let Some(denial) = approval_denial(
            entry.tool().as_ref(),
            name,
            arguments,
            &self.services.policy,
        ) {
            return Err(denial);
        }

        validate_arguments(entry.schema(), arguments).map_err(|error| {
            BrokerDenial::new(
                DenialCode::ContractError,
                format!(
                    "arguments do not match the tool schema: {}: {}",
                    error.field, error.message
                ),
            )
        })?;

        self.dispatch(entry.tool().as_ref(), callback).await
    }

    async fn run_hooks(&self, name: &str, arguments: &Value) -> Result<Value, BrokerDenial> {
        let Some(hooks) = &self.services.hooks else {
            return Ok(arguments.clone());
        };
        let sensitive = self
            .table
            .index_of(name)
            .and_then(|index| self.table.entries().get(index))
            .map_or_else(Vec::new, |entry| {
                entry
                    .tool()
                    .sensitive_params()
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect()
            });
        let borrowed: Vec<&str> = sensitive.iter().map(String::as_str).collect();
        let event = HookEvent::ToolCall {
            tool_name: name.to_string(),
            parameters: redact_params(arguments, &borrowed),
            user_id: self.services.policy.user_id.clone(),
            context: self.services.policy.hook_context.clone(),
        };
        match hooks.run(&event).await {
            Ok(HookOutcome::Continue {
                modified: Some(rewritten),
            }) => restore_sensitive(rewritten.as_str(), arguments, &borrowed),
            Ok(HookOutcome::Reject { .. }) => Err(BrokerDenial::new(
                DenialCode::PolicyDenied,
                "tool call rejected by hook",
            )),
            Ok(_) => Ok(arguments.clone()),
            Err(_) => Err(BrokerDenial::new(
                DenialCode::PolicyDenied,
                "tool call blocked by hook policy",
            )),
        }
    }

    async fn dispatch(
        &self,
        tool: &dyn Tool,
        callback: &Callback<'_>,
    ) -> Result<Value, BrokerDenial> {
        let Callback {
            name,
            arguments,
            job_ctx,
            ..
        } = *callback;
        let output =
            execute_resolved_tool_with_safety(tool, &self.services.safety, arguments, job_ctx)
                .await
                .map_err(|error| {
                    let message = self
                        .services
                        .safety
                        .sanitize_tool_output(name, &error.to_string())
                        .content;
                    BrokerDenial::new(DenialCode::ToolFailed, message)
                })?;

        let sanitized = self.services.safety.sanitize_tool_output(name, &output);
        let value = serde_json::from_str::<Value>(&sanitized.content)
            .unwrap_or(Value::String(sanitized.content));
        let size = serde_json::to_vec(&value)
            .map_err(|_| BrokerDenial::new(DenialCode::ContractError, "tool result is not JSON"))?
            .len();
        if size > MAX_BROKER_RESULT_BYTES {
            return Err(BrokerDenial::new(
                DenialCode::ContractError,
                "tool result exceeds the callback frame limit",
            ));
        }
        Ok(value)
    }
}

/// Parse a hook's rewritten arguments and put the redacted values back.
///
/// A hook sees `sensitive` fields as `[REDACTED]`, because it must not read a
/// secret it was not given. Dispatching its rewrite verbatim would therefore
/// replace the real value with the marker, so the original value is restored
/// for each sensitive key the caller supplied — the same contract the chat
/// preflight applies to hook rewrites.
fn restore_sensitive(
    rewritten: &str,
    original: &Value,
    sensitive: &[&str],
) -> Result<Value, BrokerDenial> {
    let mut parsed = serde_json::from_str::<Value>(rewritten).map_err(|_| {
        BrokerDenial::new(
            DenialCode::ContractError,
            "hook returned arguments that are not JSON",
        )
    })?;
    let Some(object) = parsed.as_object_mut() else {
        return Ok(parsed);
    };
    for key in sensitive {
        if let Some(value) = original.get(*key) {
            object.insert((*key).to_string(), value.clone());
        }
    }
    Ok(parsed)
}

fn approval_denial(
    tool: &dyn Tool,
    name: &str,
    arguments: &Value,
    policy: &CallerPolicy,
) -> Option<BrokerDenial> {
    match tool.requires_approval(arguments) {
        ApprovalRequirement::Never => None,
        ApprovalRequirement::Always => Some(BrokerDenial::new(
            DenialCode::ApprovalRequired,
            "this tool always requires interactive approval, which exec_code cannot mediate",
        )),
        ApprovalRequirement::UnlessAutoApproved => {
            if policy.auto_approved.contains(name) {
                None
            } else {
                Some(BrokerDenial::new(
                    DenialCode::ApprovalRequired,
                    "this tool requires interactive approval, which exec_code cannot mediate",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
