//! Directory discovery helpers for the skill registry.

use std::path::{Path, PathBuf};

use super::{LoadedSkill, MAX_DISCOVERED_SKILLS, SkillSource, SkillTrust};
use crate::skills::registry::loading::load_and_validate_skill;

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
    while let Ok(Some(entry)) = entries.next_entry().await {
        if count >= MAX_DISCOVERED_SKILLS {
            tracing::warn!(
                "Skill discovery cap reached ({} skills), skipping remaining",
                MAX_DISCOVERED_SKILLS
            );
            break;
        }

        let path = entry.path();
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) => meta,
            Err(error) => {
                tracing::debug!("Failed to stat {:?}: {}", path, error);
                continue;
            }
        };

        if meta.is_symlink() {
            tracing::warn!(
                "Skipping symlink in skills directory: {:?}",
                path.file_name().unwrap_or_default()
            );
            continue;
        }

        if meta.is_dir() {
            let skill_md = path.join("SKILL.md");
            if tokio::fs::try_exists(&skill_md).await.unwrap_or(false) {
                count += 1;
                let source = make_source(path.clone());
                match load_and_validate_skill(&skill_md, trust, source).await {
                    Ok((name, skill)) => {
                        tracing::debug!("Loaded skill: {}", name);
                        results.push((name, skill));
                    }
                    Err(error) => {
                        tracing::warn!(
                            "Failed to load skill from {:?}: {}",
                            path.file_name().unwrap_or_default(),
                            error
                        );
                    }
                }
            }
            continue;
        }

        if meta.is_file()
            && let Some(file_name) = path.file_name().and_then(|file| file.to_str())
            && file_name == "SKILL.md"
        {
            count += 1;
            let source = make_source(path.clone());
            match load_and_validate_skill(&path, trust, source).await {
                Ok((name, skill)) => {
                    tracing::info!("Loaded skill: {}", name);
                    results.push((name, skill));
                }
                Err(error) => {
                    tracing::warn!("Failed to load skill from {:?}: {}", file_name, error);
                }
            }
        }
    }

    results
}
