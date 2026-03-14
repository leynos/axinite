//! Hosted remote-tool policy and execution helpers for worker requests.
//!
//! This module keeps the hosted-worker remote-tool predicate separate from the
//! HTTP adapter so the catalog and execute endpoints share one policy surface.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::http::StatusCode;
use uuid::Uuid;

use crate::context::JobContext;
use crate::llm::ToolDefinition;
use crate::tools::{Tool, ToolDomain, ToolError, ToolOutput, ToolRegistry};

enum HostedRemoteToolEligibility {
    Eligible,
    ApprovalGated,
    Ineligible,
}

pub(super) async fn hosted_remote_tool_catalog(
    tools: &Arc<ToolRegistry>,
) -> (Vec<ToolDefinition>, Vec<String>, u64) {
    let mut hosted_tools = tools
        .all()
        .await
        .into_iter()
        .filter(|tool| {
            matches!(
                hosted_remote_tool_eligibility(tool),
                HostedRemoteToolEligibility::Eligible
            )
        })
        .map(|tool| ToolDefinition {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters: tool.parameters_schema(),
        })
        .collect::<Vec<_>>();
    hosted_tools.sort_unstable_by(|left, right| left.name.cmp(&right.name));

    let toolset_instructions = Vec::new();
    let catalog_version = compute_catalog_version(&hosted_tools, &toolset_instructions);
    (hosted_tools, toolset_instructions, catalog_version)
}

pub(super) async fn execute_hosted_remote_tool(
    tools: &Arc<ToolRegistry>,
    user_id: &str,
    job_id: Uuid,
    tool_name: &str,
    params: serde_json::Value,
) -> Result<ToolOutput, StatusCode> {
    let tool = tools.get(tool_name).await.ok_or(StatusCode::NOT_FOUND)?;
    match hosted_remote_tool_eligibility(&tool) {
        HostedRemoteToolEligibility::Eligible => {}
        HostedRemoteToolEligibility::ApprovalGated => {
            tracing::warn!(
                job_id = %job_id,
                tool = %tool_name,
                "Worker attempted approval-gated hosted remote tool execution"
            );
            return Err(StatusCode::FORBIDDEN);
        }
        HostedRemoteToolEligibility::Ineligible => {
            tracing::warn!(
                job_id = %job_id,
                tool = %tool_name,
                "Worker attempted non-hosted remote tool execution"
            );
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    if tool.requires_approval(&params).is_required() {
        tracing::warn!(
            job_id = %job_id,
            tool = %tool_name,
            "Worker attempted approval-gated hosted remote tool execution"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    let mut ctx = JobContext::with_user(
        user_id.to_string(),
        "Hosted remote tool",
        format!("Hosted execution of {}", tool_name),
    );
    ctx.job_id = job_id;

    tool.execute(params, &ctx).await.map_err(|error| {
        tracing::warn!(
            job_id = %job_id,
            tool = %tool_name,
            error = %error,
            "Hosted remote tool execution failed"
        );
        tool_error_status(&error)
    })
}

fn hosted_remote_tool_eligibility(tool: &Arc<dyn Tool>) -> HostedRemoteToolEligibility {
    if tool.domain() != ToolDomain::Orchestrator
        || ToolRegistry::is_protected_tool_name(tool.name())
    {
        HostedRemoteToolEligibility::Ineligible
    } else if tool.requires_approval(&serde_json::json!({})).is_required() {
        HostedRemoteToolEligibility::ApprovalGated
    } else {
        HostedRemoteToolEligibility::Eligible
    }
}

fn compute_catalog_version(tools: &[ToolDefinition], toolset_instructions: &[String]) -> u64 {
    let mut hasher = DefaultHasher::new();
    serde_json::json!({
        "tools": tools
            .iter()
            .map(|tool| serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
            }))
            .collect::<Vec<_>>(),
        "instructions": toolset_instructions,
    })
    .to_string()
    .hash(&mut hasher);
    hasher.finish()
}

fn tool_error_status(error: &ToolError) -> StatusCode {
    match error {
        ToolError::InvalidParameters(_) => StatusCode::BAD_REQUEST,
        ToolError::NotAuthorized(_) => StatusCode::FORBIDDEN,
        ToolError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::BAD_GATEWAY,
    }
}
