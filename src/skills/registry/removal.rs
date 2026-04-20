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
    let Some(metadata) = check_path_metadata(path).await? else {
        return Ok(());
    };
    perform_delete(path, &metadata).await
}

async fn check_path_metadata(path: &Path) -> Result<Option<std::fs::Metadata>, SkillRegistryError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SkillRegistryError::WriteError {
            path: path.display().to_string(),
            reason: error.to_string(),
        }),
    }
}

async fn perform_delete(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), SkillRegistryError> {
    let result = if metadata.is_file() {
        tokio::fs::remove_file(path).await
    } else {
        tokio::fs::remove_dir_all(path).await
    };
    map_delete_result(path, result)
}

fn map_delete_result(path: &Path, result: std::io::Result<()>) -> Result<(), SkillRegistryError> {
    match result {
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
