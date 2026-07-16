//! File operation tools for reading, writing, and navigating the filesystem.
//!
//! These tools provide controlled access to the filesystem with:
//! - Path validation and sandboxing
//! - Size limits on read/write operations
//! - Support for common development tasks
//!
//! ## Module layout
//!
//! - [`read`] — the `read_file` tool
//! - [`write`] — the `write_file` tool and workspace-path rejection
//! - [`list`] — the `list_dir` tool and listing helpers
//! - [`patch`] — the `apply_patch` search/replace tool

mod list;
mod patch;
mod read;
mod write;

#[cfg(test)]
mod tests;

pub use list::ListDirTool;
pub use patch::ApplyPatchTool;
pub use read::ReadFileTool;
pub use write::WriteFileTool;
