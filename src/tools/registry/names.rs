//! Protected-name helpers for the tool registry.

/// Names of built-in tools that cannot be shadowed by dynamic registrations.
/// This prevents a dynamically built or installed tool from replacing a
/// security-critical built-in like "shell" or "memory_write".
pub const PROTECTED_TOOL_NAMES: &[&str] = &[
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
    "job_status",
    "job_events",
    "job_prompt",
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

pub fn is_protected_tool_name(name: &str) -> bool {
    PROTECTED_TOOL_NAMES.contains(&name)
}
