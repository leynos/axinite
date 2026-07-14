//! The `list_dir` tool: bounded, optionally recursive directory listings.

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::context::JobContext;
use crate::tools::builtin::path_utils::validate_path;
use crate::tools::tool::{ApprovalRequirement, NativeTool, ToolDomain, ToolError, ToolOutput};

/// Maximum directory listing entries.
const MAX_DIR_ENTRIES: usize = 500;

/// List directory contents tool.
#[derive(Debug, Default)]
pub struct ListDirTool {
    base_dir: Option<PathBuf>,
}

impl ListDirTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = Some(dir);
        self
    }
}

impl NativeTool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List contents of a directory on the LOCAL FILESYSTEM. NOT for workspace memory \
         (use memory_tree for that). Shows files and subdirectories with their sizes."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list (defaults to current directory)"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "If true, list contents recursively (default false)"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum depth for recursive listing (default 3)"
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let path_str = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let recursive = params
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let max_depth = params
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        let start = std::time::Instant::now();

        let path = validate_path(path_str, self.base_dir.as_deref())?;

        let mut entries = Vec::new();
        list_dir_inner(&path, &path, recursive, max_depth, 0, &mut entries).await?;

        // Sort entries
        entries.sort_by(|a, b| {
            let a_is_dir = a.ends_with('/');
            let b_is_dir = b.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });

        let truncated = entries.len() > MAX_DIR_ENTRIES;
        if truncated {
            entries.truncate(MAX_DIR_ENTRIES);
        }

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "entries": entries,
            "count": entries.len(),
            "truncated": truncated
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false // Directory listings are safe
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

/// Recursively list directory contents.
async fn list_dir_inner(
    base: &Path,
    path: &Path,
    recursive: bool,
    max_depth: usize,
    current_depth: usize,
    entries: &mut Vec<String>,
) -> Result<(), ToolError> {
    if entries.len() >= MAX_DIR_ENTRIES {
        return Ok(());
    }

    let mut dir = fs::read_dir(path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read directory: {}", e)))?;

    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read entry: {}", e)))?
    {
        if entries.len() >= MAX_DIR_ENTRIES {
            break;
        }

        let entry_path = entry.path();
        let metadata = entry
            .metadata()
            .await
            .ok()
            .map(ambient_fs::Metadata::from_std);
        let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());

        entries.push(render_entry(base, &entry_path, metadata.as_ref(), is_dir));

        if !should_recurse(recursive, is_dir, current_depth, max_depth) {
            continue;
        }
        if is_excluded_dir(&entry.file_name()) {
            continue;
        }
        Box::pin(list_dir_inner(
            base,
            &entry_path,
            recursive,
            max_depth,
            current_depth + 1,
            entries,
        ))
        .await?;
    }

    Ok(())
}

/// Render a single listing line: directories end with `/`, files carry a
/// human-readable size.
fn render_entry(
    base: &Path,
    entry_path: &Path,
    metadata: Option<&ambient_fs::Metadata>,
    is_dir: bool,
) -> String {
    let relative = entry_path
        .strip_prefix(base)
        .unwrap_or(entry_path)
        .to_string_lossy();

    if is_dir {
        format!("{}/", relative)
    } else {
        let size = metadata.map(|m| m.len()).unwrap_or(0);
        format!("{} ({})", relative, format_size(size))
    }
}

/// Whether a directory name is a common non-essential directory that
/// recursive listings skip (build outputs, caches, virtual environments).
fn is_excluded_dir(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_string_lossy().as_ref(),
        "node_modules" | "target" | ".git" | "__pycache__" | "venv" | ".venv"
    )
}

/// Whether a directory listing should descend into a subdirectory.
fn should_recurse(recursive: bool, is_dir: bool, current_depth: usize, max_depth: usize) -> bool {
    let descend_into_dir = recursive && is_dir;
    descend_into_dir && current_depth < max_depth
}

/// Format file size in human-readable form.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}
