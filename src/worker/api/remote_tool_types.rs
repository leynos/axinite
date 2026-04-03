//! Remote-tool transport types shared between worker and orchestrator.
//!
//! This module defines the serializable payloads used for hosted remote tool
//! definitions and execution outputs, where [`ToolDefinition`] describes
//! visible tools and [`ToolOutput`] carries execution results.

use serde::{Deserialize, Serialize};

use crate::llm::ToolDefinition;
use crate::tools::ToolOutput;

/// Request sent from a worker to the orchestrator for hosted remote-tool execution.
///
/// `tool_name` is the orchestrator tool identifier. `params` must match that
/// tool's JSON Schema because the orchestrator validates and executes the call.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteToolExecutionRequest {
    /// Stable hosted remote-tool identifier known to both worker and orchestrator.
    pub tool_name: String,
    /// JSON parameters passed through to the tool implementation.
    pub params: serde_json::Value,
}

/// Response returned after the orchestrator executes a hosted remote tool.
///
/// `output` is the tool's `ToolOutput`, including its result payload and
/// reported side-effect metadata such as duration and optional cost.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteToolExecutionResponse {
    /// Tool execution output returned by the orchestrator.
    pub output: ToolOutput,
}

/// Catalogue payload returned to workers for hosted-visible remote tools.
///
/// `tools` is the current model-facing tool list. `toolset_instructions` is
/// optional human-readable guidance and defaults to an empty list.
/// `catalog_version` is a deterministic content version derived from the
/// serialized catalogue payload.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteToolCatalogResponse {
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub toolset_instructions: Vec<String>,
    pub catalog_version: u64,
}
