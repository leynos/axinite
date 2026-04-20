//! Removal helpers for installed skills.

use std::path::{Path, PathBuf};

use super::{SkillRegistry, SkillRegistryError, SkillSource};

pub(super) fn validate_remove(
    registry: &SkillRegistry,
    name: &str,
) -> Result<PathBuf, SkillRegistryError> {
    let idx = registry
        .skills
        .iter()
        .position(|skill| skill.manifest.name == name)
        .ok_or_else(|| SkillRegistryError::NotFound(name.to_string()))?;

    let skill = &registry.skills[idx];
    match &skill.source {
        SkillSource::User(path) => Ok(path.clone()),
        SkillSource::Workspace(_) => Err(SkillRegistryError::CannotRemove {
            name: name.to_string(),
            reason: "workspace skills cannot be removed via this interface".to_string(),
        }),
        SkillSource::Bundled(_) => Err(SkillRegistryError::CannotRemove {
            name: name.to_string(),
            reason: "bundled skills cannot be removed".to_string(),
        }),
    }
}

pub(super) async fn delete_skill_files(path: &Path) -> Result<(), SkillRegistryError> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SkillRegistryError::WriteError {
            path: path.display().to_string(),
            reason: error.to_string(),
        }),
    }
}

pub(super) fn commit_remove(
    registry: &mut SkillRegistry,
    name: &str,
) -> Result<(), SkillRegistryError> {
    let idx = registry
        .skills
        .iter()
        .position(|skill| skill.manifest.name == name)
        .ok_or_else(|| SkillRegistryError::NotFound(name.to_string()))?;

    registry.skills.remove(idx);
    tracing::info!("Removed skill: {}", name);
    Ok(())
}
