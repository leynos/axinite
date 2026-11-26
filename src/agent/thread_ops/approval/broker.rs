//! Issuing `exec_code` authority after an approval.
//!
//! A pending request carries the caller's attenuated ceiling and auto-approval
//! snapshot, so an approved call rebuilds the same authority the dispatcher
//! would have issued before the pause. Outer approval never authorizes a
//! nested tool: the broker still re-checks every callback.

use std::sync::Arc;

use crate::agent::Agent;
use crate::agent::session::PendingApproval;
use crate::error::Error;
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;
use crate::tools::monty::broker::{BrokerContext, BrokerServices, CallerPolicy};

/// Rebuild the authority an approved `exec_code` call runs with.
///
/// # Errors
///
/// Returns an invalid-parameter error when the request names a tool outside
/// the caller's ceiling, or one the registry no longer resolves.
pub(super) async fn approved_broker(
    agent: &Agent,
    user_id: &str,
    pending: &PendingApproval,
) -> Result<BrokerContext, Error> {
    let requested = requested_tools(pending);
    let policy = CallerPolicy {
        user_id: user_id.to_string(),
        hook_context: "chat".to_string(),
        auto_approved: pending.auto_approved_tools.iter().cloned().collect(),
        permitted: Some(pending.permitted_tools.iter().cloned().collect()),
    };
    let services = BrokerServices {
        tools: Arc::clone(agent.tools()) as Arc<ToolRegistry>,
        safety: Arc::clone(agent.safety()) as Arc<SafetyLayer>,
        hooks: Some(Arc::clone(agent.hooks())),
        policy,
    };
    BrokerContext::new(services, &requested)
        .await
        .map_err(|error| {
            crate::error::ToolError::InvalidParameters {
                name: "exec_code".to_string(),
                reason: error.to_string(),
            }
            .into()
        })
}

/// Read the tool names an approved `exec_code` call may expose.
fn requested_tools(pending: &PendingApproval) -> Vec<String> {
    pending
        .parameters
        .get("allowed_tools")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
