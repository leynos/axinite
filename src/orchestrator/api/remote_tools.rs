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
use crate::tools::{
    HostedToolCatalogSource, HostedToolLookupError, Tool, ToolError, ToolOutput, ToolRegistry,
};

const HOSTED_REMOTE_TOOL_SOURCES: [HostedToolCatalogSource; 2] =
    [HostedToolCatalogSource::Mcp, HostedToolCatalogSource::Wasm];

/// Request context for executing a hosted-eligible remote tool.
///
/// `user_id` identifies the worker's effective user, `job_id` identifies the
/// active hosted job, `tool_name` selects the orchestrator-owned tool to run,
/// and `params` carries the JSON arguments forwarded to that tool.
pub(super) struct HostedRemoteToolRequest {
    pub user_id: String,
    pub job_id: Uuid,
    pub tool_name: String,
    pub params: serde_json::Value,
}

/// Build the hosted-worker remote-tool catalogue from the orchestrator registry.
///
/// Returns the hosted-visible tool definitions, any toolset instructions that
/// should be injected into the worker prompt, and the deterministic catalogue
/// version hash for that tool/instruction set.
pub(super) async fn hosted_remote_tool_catalog(
    tools: &Arc<ToolRegistry>,
) -> (Vec<ToolDefinition>, Vec<String>, u64) {
    let mut hosted_tools = tools
        .hosted_tool_definitions(&HOSTED_REMOTE_TOOL_SOURCES)
        .await;
    hosted_tools.sort_by(|a, b| a.name.cmp(&b.name));
    let toolset_instructions = Vec::new();
    let catalog_version = compute_catalog_version(&hosted_tools, &toolset_instructions);
    (hosted_tools, toolset_instructions, catalog_version)
}

/// Execute a hosted-eligible orchestrator tool for a worker request.
///
/// Returns the tool output on success, or an HTTP `StatusCode` describing why
/// the worker request was rejected or why execution failed.
pub(super) async fn execute_hosted_remote_tool(
    tools: &Arc<ToolRegistry>,
    request: HostedRemoteToolRequest,
) -> Result<ToolOutput, StatusCode> {
    let HostedRemoteToolRequest {
        user_id,
        job_id,
        tool_name,
        params,
    } = request;
    let tool = resolve_hosted_tool(tools, &tool_name, job_id).await?;

    if tool.requires_approval(&params).is_required() {
        tracing::warn!(
            job_id = %job_id,
            tool = %tool_name,
            "Worker attempted approval-gated hosted remote tool execution"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    let mut ctx = JobContext::with_user(
        user_id,
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

async fn resolve_hosted_tool(
    tools: &Arc<ToolRegistry>,
    tool_name: &str,
    job_id: Uuid,
) -> Result<Arc<dyn Tool>, StatusCode> {
    match tools
        .get_hosted_tool(tool_name, &HOSTED_REMOTE_TOOL_SOURCES)
        .await
    {
        Ok(tool) => Ok(tool),
        Err(HostedToolLookupError::NotFound) => Err(StatusCode::NOT_FOUND),
        Err(HostedToolLookupError::ApprovalGated) => {
            tracing::warn!(
                job_id = %job_id,
                tool = %tool_name,
                "Worker attempted approval-gated hosted remote tool execution"
            );
            Err(StatusCode::FORBIDDEN)
        }
        Err(HostedToolLookupError::Ineligible) => {
            tracing::warn!(
                job_id = %job_id,
                tool = %tool_name,
                "Worker attempted non-hosted remote tool execution"
            );
            Err(StatusCode::BAD_REQUEST)
        }
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
