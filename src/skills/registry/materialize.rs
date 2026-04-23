//! Install-artifact materialisation for staged skill installs.

use std::path::{Path, PathBuf};

use super::{SkillInstallPayload, SkillRegistryError};
use crate::skills::bundle::{
    ValidatedSkillBundle, looks_like_skill_archive, validate_skill_archive,
};
use crate::skills::normalize_line_endings;
use crate::skills::parser::{SkillParseError, parse_skill_md};

pub(super) struct InstallArtifact {
    pub(super) install_dir_name: String,
    pub(super) files: Vec<(PathBuf, Vec<u8>)>,
}

pub(super) fn materialize_install_artifact(
    payload: SkillInstallPayload,
) -> Result<InstallArtifact, SkillRegistryError> {
    match payload {
        SkillInstallPayload::Markdown(content) => build_markdown_artifact(content),
        SkillInstallPayload::DownloadedBytes(bytes) => {
            if looks_like_skill_archive(&bytes) {
                Ok(build_bundle_artifact(validate_skill_archive(&bytes)?))
            } else {
                let content = String::from_utf8(bytes).map_err(|error| {
                    SkillRegistryError::InvalidContent {
                        reason: format!("downloaded skill content is not valid UTF-8: {error}"),
                    }
                })?;
                build_markdown_artifact(content)
            }
        }
    }
}

fn build_markdown_artifact(content: String) -> Result<InstallArtifact, SkillRegistryError> {
    let normalized_content = normalize_line_endings(&content);
    let parsed = parse_skill_md(&normalized_content).map_err(|e: SkillParseError| match e {
        SkillParseError::InvalidName { ref name } => SkillRegistryError::ParseError {
            name: name.clone(),
            reason: e.to_string(),
        },
        _ => SkillRegistryError::ParseError {
            name: "(install)".to_string(),
            reason: e.to_string(),
        },
    })?;

    Ok(InstallArtifact {
        install_dir_name: parsed.manifest.name,
        files: vec![(PathBuf::from("SKILL.md"), normalized_content.into_bytes())],
    })
}

fn build_bundle_artifact(bundle: ValidatedSkillBundle) -> InstallArtifact {
    let files = bundle
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.relative_path().to_path_buf(),
                entry.contents().to_vec(),
            )
        })
        .collect();

    InstallArtifact {
        install_dir_name: bundle.skill_name().to_string(),
        files,
    }
}

pub(super) async fn write_install_artifact(
    staged_dir: &Path,
    artifact: &InstallArtifact,
) -> Result<(), SkillRegistryError> {
    for (relative_path, contents) in &artifact.files {
        let file_path = staged_dir.join(relative_path);
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                SkillRegistryError::WriteError {
                    path: parent.display().to_string(),
                    reason: e.to_string(),
                }
            })?;
        }

        tokio::fs::write(&file_path, contents).await.map_err(|e| {
            SkillRegistryError::WriteError {
                path: file_path.display().to_string(),
                reason: e.to_string(),
            }
        })?;
    }

    Ok(())
}
