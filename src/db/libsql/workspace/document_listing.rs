//! Listing helpers for libSQL workspace document queries.

use std::collections::HashMap;

use crate::workspace::WorkspaceEntry;

pub(super) fn normalise_dir_prefix(directory: &str) -> String {
    if !directory.is_empty() && !directory.ends_with('/') {
        format!("{}/", directory)
    } else {
        directory.to_string()
    }
}

pub(super) fn dir_like_pattern(dir: &str) -> String {
    if dir.is_empty() {
        "%".to_string()
    } else {
        format!("{}%", dir)
    }
}

pub(super) fn resolve_entry(full_path: &str, dir: &str) -> Option<(String, bool, String)> {
    let relative = if dir.is_empty() {
        full_path
    } else {
        full_path.strip_prefix(dir)?
    };
    let child_name = if let Some(slash_pos) = relative.find('/') {
        &relative[..slash_pos]
    } else {
        relative
    };
    if child_name.is_empty() {
        return None;
    }
    let is_dir = relative.contains('/');
    let entry_path = if dir.is_empty() {
        child_name.to_string()
    } else {
        format!("{}{}", dir, child_name)
    };
    Some((child_name.to_string(), is_dir, entry_path))
}

pub(super) fn merge_entry(
    entries_map: &mut HashMap<String, WorkspaceEntry>,
    child_name: String,
    entry_path: String,
    is_dir: bool,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    content_preview: Option<String>,
) {
    entries_map
        .entry(child_name)
        .and_modify(|entry| {
            if is_dir {
                entry.is_directory = true;
                entry.content_preview = None;
            }
            if let (Some(existing), Some(new)) = (&entry.updated_at, &updated_at)
                && new > existing
            {
                entry.updated_at = Some(*new);
            }
        })
        .or_insert(WorkspaceEntry {
            path: entry_path,
            is_directory: is_dir,
            updated_at,
            content_preview: if is_dir { None } else { content_preview },
        });
}
