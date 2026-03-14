//! Tool registry for managing available tools.

mod extension;
mod job;
#[cfg(test)]
mod tests;
mod wasm;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::llm::{LlmProvider, ToolDefinition};
use crate::secrets::SecretsStore;
use crate::tools::builder::{BuildSoftwareTool, BuilderConfig, LlmSoftwareBuilder};
use crate::tools::builtin::{
    ApplyPatchTool, EchoTool, HttpTool, JsonTool, ListDirTool, MemoryReadTool, MemorySearchTool,
    MemoryTreeTool, MemoryWriteTool, ReadFileTool, ShellTool, TimeTool, WriteFileTool,
};
use crate::tools::rate_limiter::RateLimiter;
use crate::tools::tool::{Tool, ToolDomain};
use crate::tools::wasm::SharedCredentialRegistry;
use crate::workspace::Workspace;

pub use extension::{ImageToolsArgs, VisionToolsArgs};
pub use job::RegisterJobToolsConfig;
pub use wasm::{WasmFromStorageArgs, WasmRegistrationError, WasmToolRegistration};

/// Names of built-in tools that cannot be shadowed by dynamic registrations.
/// This prevents a dynamically built or installed tool from replacing a
/// security-critical built-in like "shell" or "memory_write".
const PROTECTED_TOOL_NAMES: &[&str] = &[
    "echo",
    "time",
    "json",
    "http",
    "shell",
    "read_file",
    "write_file",
    "list_dir",
    "apply_patch",
    "memory_search",
    "memory_write",
    "memory_read",
    "memory_tree",
    "create_job",
    "list_jobs",
    "job_events",
    "job_prompt",
    "job_status",
    "cancel_job",
    "build_software",
    "tool_search",
    "tool_install",
    "tool_auth",
    "tool_activate",
    "tool_list",
    "tool_upgrade",
    "extension_info",
    "tool_remove",
    "routine_create",
    "routine_list",
    "routine_update",
    "routine_delete",
    "routine_fire",
    "routine_history",
    "event_emit",
    "skill_list",
    "skill_search",
    "skill_install",
    "skill_remove",
    "message",
    "web_fetch",
    "restart",
    "image_generate",
    "image_edit",
    "image_analyze",
];

/// Registry of available tools.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    /// Tracks which names were registered as built-in (protected from shadowing).
    builtin_names: RwLock<std::collections::HashSet<String>>,
    /// Shared credential registry populated by WASM tools, consumed by HTTP tool.
    credential_registry: Option<Arc<SharedCredentialRegistry>>,
    /// Secrets store for credential injection (shared with HTTP tool).
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// Shared rate limiter for built-in tool invocations.
    rate_limiter: RateLimiter,
    /// Reference to the message tool for setting context per-turn.
    message_tool: RwLock<Option<Arc<crate::tools::builtin::MessageTool>>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("count", &self.count())
            .finish()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("count", &self.count())
            .finish()
    }
}
