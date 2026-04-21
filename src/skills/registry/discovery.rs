//! Directory discovery helpers for the skill registry.

use std::path::{Path, PathBuf};

use super::{LoadedSkill, MAX_DISCOVERED_SKILLS, SkillSource, SkillTrust};
use crate::skills::registry::loading::load_and_validate_skill;

enum EntryLoadResult {
    /// Not a skill candidate; do not increment the load counter.
    NotASkill,
    /// Was a skill candidate but failed to load; increment the counter.
    LoadFailed,
    /// Loaded successfully; increment the counter and keep the skill.
    Loaded(String, Box<LoadedSkill>),
}

pub(super) async fn discover_from_dir<F>(
    dir: &Path,
    trust: SkillTrust,
    make_source: F,
) -> Vec<(String, LoadedSkill)>
where
    F: Fn(PathBuf) -> SkillSource,
{
    let mut results = Vec::new();

    if !tokio::fs::try_exists(dir).await.unwrap_or(false) {
        tracing::debug!("Skills directory does not exist: {:?}", dir);
        return results;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!("Failed to read skills directory {:?}: {}", dir, error);
            return results;
        }
    };

    let mut count = 0usize;
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!("Error iterating skills directory {:?}: {}", dir, error);
                break;
            }
        };

        if count >= MAX_DISCOVERED_SKILLS {
            tracing::warn!(
                "Skill discovery cap reached ({} skills), skipping remaining",
                MAX_DISCOVERED_SKILLS
            );
            break;
        }

        match classify_entry(&entry, dir, trust, &make_source).await {
            EntryLoadResult::NotASkill => {}
            EntryLoadResult::LoadFailed => {
                count += 1;
            }
            EntryLoadResult::Loaded(name, skill) => {
                count += 1;
                results.push((name, *skill));
            }
        }
    }

    results
}

/// Returns the file name as `&str` when `path` is a regular file
/// named exactly `SKILL.md`.
fn flat_skill_md_name<'a>(meta: &std::fs::Metadata, path: &'a Path) -> Option<&'a str> {
    if !meta.is_file() {
        return None;
    }
    path.file_name()
        .and_then(|f| f.to_str())
        .filter(|&name| name == "SKILL.md")
}

async fn classify_entry<F>(
    entry: &tokio::fs::DirEntry,
    _dir: &Path,
    trust: SkillTrust,
    make_source: &F,
) -> EntryLoadResult
where
    F: Fn(PathBuf) -> SkillSource,
{
    let path = entry.path();
    let meta = match tokio::fs::symlink_metadata(&path).await {
        Ok(meta) => meta,
        Err(error) => {
            tracing::debug!("Failed to stat {:?}: {}", path, error);
            return EntryLoadResult::NotASkill;
        }
    };

    if meta.is_symlink() {
        tracing::warn!(
            "Skipping symlink in skills directory: {:?}",
            path.file_name().unwrap_or_default()
        );
        return EntryLoadResult::NotASkill;
    }

    if meta.is_dir() {
        return try_load_from_subdir(&path, trust, make_source).await;
    }

    if let Some(file_name) = flat_skill_md_name(&meta, &path) {
        return try_load_flat_skill(&path, file_name, trust, make_source(path.clone())).await;
    }

    EntryLoadResult::NotASkill
}

async fn try_load_from_subdir<F>(path: &Path, trust: SkillTrust, make_source: &F) -> EntryLoadResult
where
    F: Fn(PathBuf) -> SkillSource,
{
    let skill_md = path.join("SKILL.md");
    if !tokio::fs::try_exists(&skill_md).await.unwrap_or(false) {
        return EntryLoadResult::NotASkill;
    }

    let source = make_source(path.to_path_buf());
    match load_and_validate_skill(&skill_md, trust, source).await {
        Ok((name, skill)) => {
            tracing::debug!("Loaded skill: {}", name);
            EntryLoadResult::Loaded(name, Box::new(skill))
        }
        Err(error) => {
            tracing::warn!(
                "Failed to load skill from {:?}: {}",
                path.file_name().unwrap_or_default(),
                error
            );
            EntryLoadResult::LoadFailed
        }
    }
}

async fn try_load_flat_skill(
    path: &Path,
    file_name: &str,
    trust: SkillTrust,
    source: SkillSource,
) -> EntryLoadResult {
    match load_and_validate_skill(path, trust, source).await {
        Ok((name, skill)) => {
            tracing::info!("Loaded skill: {}", name);
            EntryLoadResult::Loaded(name, Box::new(skill))
        }
        Err(error) => {
            tracing::warn!("Failed to load skill from {:?}: {}", file_name, error);
            EntryLoadResult::LoadFailed
        }
    }
}
