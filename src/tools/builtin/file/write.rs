//! The `write_file` tool: size-limited writes with workspace-path rejection.

use std::path::PathBuf;

use tokio::fs;

use crate::context::JobContext;
use crate::tools::builtin::path_utils::validate_path;
use crate::tools::tool::{
    ApprovalRequirement, NativeTool, ToolDomain, ToolError, ToolOutput, require_str,
};
use crate::workspace::paths as ws_paths;

/// Well-known workspace filenames that must go through memory_write, not write_file.
///
/// If the LLM tries to write one of these via the filesystem tool we reject
/// immediately and point it at the correct tool.
const WORKSPACE_FILES: &[&str] = &[
    ws_paths::HEARTBEAT,
    ws_paths::MEMORY,
    ws_paths::IDENTITY,
    ws_paths::SOUL,
    ws_paths::AGENTS,
    ws_paths::USER,
    ws_paths::README,
];

/// Check whether `path` resolves to a workspace file that should be written
/// through `memory_write` instead of `write_file`.
fn is_workspace_path(path: &str) -> bool {
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(path);

    WORKSPACE_FILES.contains(&filename)
        || path.starts_with("daily/")
        || path.starts_with("context/")
}

/// Maximum file size for writing (5MB).
const MAX_WRITE_SIZE: usize = 5 * 1024 * 1024;

/// Write file contents tool.
#[derive(Debug, Default)]
pub struct WriteFileTool {
    base_dir: Option<PathBuf>,
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = Some(dir);
        self
    }
}

impl NativeTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file on the LOCAL FILESYSTEM. NOT for workspace memory \
         (use memory_write for that). Creates the file if it doesn't exist, overwrites if it does. \
         Parent directories are created automatically. Use apply_patch for targeted edits."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let path_str = require_str(&params, "path")?;

        // Reject workspace paths: these live in the database, not on disk.
        if is_workspace_path(path_str) {
            return Err(ToolError::InvalidParameters(format!(
                "'{}' is a workspace memory file. Use the memory_write tool instead of write_file. \
                 For HEARTBEAT.md use target='heartbeat', for MEMORY.md use target='memory'.",
                path_str
            )));
        }

        let content = require_str(&params, "content")?;

        let start = std::time::Instant::now();

        // Check content size
        if content.len() > MAX_WRITE_SIZE {
            return Err(ToolError::InvalidParameters(format!(
                "Content too large ({} bytes). Maximum is {} bytes.",
                content.len(),
                MAX_WRITE_SIZE
            )));
        }

        let path = validate_path(path_str, self.base_dir.as_deref())?;

        // Create parent directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to create directories: {}", e))
            })?;
        }

        // Write file
        fs::write(&path, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "bytes_written": content.len(),
            "success": true
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn requires_sanitization(&self) -> bool {
        false // We're writing, not reading external data
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }

    fn rate_limit_config(&self) -> Option<crate::tools::tool::ToolRateLimitConfig> {
        Some(crate::tools::tool::ToolRateLimitConfig::new(20, 200))
    }
}
