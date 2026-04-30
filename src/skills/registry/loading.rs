//! Skill loading and validation helpers for the registry.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::{SkillRegistryError, to_lowercase_vec};
use crate::skills::gating;
use crate::skills::parser::{SkillParseError, parse_skill_md};
use crate::skills::{
    GatingRequirements, LoadedSkill, LoadedSkillLocation, MAX_PROMPT_FILE_SIZE, SkillPackageKind,
    SkillSource, SkillTrust, normalize_line_endings,
};

/// Load and validate a single SKILL.md file from disk.
///
/// Shared implementation used by both `SkillRegistry::load_skill_md` (discovery)
/// and `SkillRegistry::prepare_install_to_disk` (installation). This avoids
/// duplicating the read/parse/validate/hash pipeline.
pub(super) async fn load_and_validate_skill(
    path: &Path,
    trust: SkillTrust,
    source: SkillSource,
    root: &Path,
    package_kind: SkillPackageKind,
) -> Result<(String, LoadedSkill), SkillRegistryError> {
    let raw_bytes = read_skill_bytes(path).await?;

    let raw_content = String::from_utf8(raw_bytes).map_err(|e| SkillRegistryError::ReadError {
        path: path.display().to_string(),
        reason: format!("Invalid UTF-8: {e}"),
    })?;

    let normalized_content = normalize_line_endings(&raw_content);
    let parsed = parse_skill_md(&normalized_content).map_err(|e| map_parse_error(e, path))?;

    let manifest = parsed.manifest;
    let prompt_content = parsed.prompt_content;

    let openclaw_requires = manifest
        .metadata
        .as_ref()
        .and_then(|meta| meta.openclaw.as_ref())
        .map(|oc| &oc.requires);
    check_openclaw_gating(&manifest.name, openclaw_requires).await?;

    let approx_tokens = (prompt_content.len() as f64 * 0.25) as usize;
    let declared = manifest.activation.max_context_tokens;
    check_token_budget(&manifest.name, approx_tokens, declared)?;

    let content_hash = compute_hash(&prompt_content);
    let compiled_patterns = LoadedSkill::compile_patterns(&manifest.activation.patterns);
    let lowercased_keywords = to_lowercase_vec(&manifest.activation.keywords);
    let lowercased_exclude_keywords = to_lowercase_vec(&manifest.activation.exclude_keywords);
    let lowercased_tags = to_lowercase_vec(&manifest.activation.tags);

    let name = manifest.name.clone();
    let location = LoadedSkillLocation::new(
        name.clone(),
        root.to_path_buf(),
        Path::new("SKILL.md").to_path_buf(),
        package_kind,
    );
    let skill = LoadedSkill {
        manifest,
        prompt_content,
        trust,
        source,
        location,
        content_hash,
        compiled_patterns,
        lowercased_keywords,
        lowercased_exclude_keywords,
        lowercased_tags,
    };

    Ok((name, skill))
}

async fn read_skill_bytes(path: &Path) -> Result<Vec<u8>, SkillRegistryError> {
    let file_meta =
        tokio::fs::symlink_metadata(path)
            .await
            .map_err(|e| SkillRegistryError::ReadError {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

    if file_meta.is_symlink() {
        return Err(SkillRegistryError::SymlinkDetected {
            path: path.display().to_string(),
        });
    }

    let raw_bytes = tokio::fs::read(path)
        .await
        .map_err(|e| SkillRegistryError::ReadError {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

    if raw_bytes.len() as u64 > MAX_PROMPT_FILE_SIZE {
        return Err(SkillRegistryError::FileTooLarge {
            name: path.display().to_string(),
            size: raw_bytes.len() as u64,
            max: MAX_PROMPT_FILE_SIZE,
        });
    }

    Ok(raw_bytes)
}

fn map_parse_error(e: SkillParseError, path: &Path) -> SkillRegistryError {
    match e {
        SkillParseError::InvalidName { ref name } => SkillRegistryError::ParseError {
            name: name.clone(),
            reason: e.to_string(),
        },
        _ => SkillRegistryError::ParseError {
            name: path.display().to_string(),
            reason: e.to_string(),
        },
    }
}

async fn check_openclaw_gating(
    name: &str,
    openclaw_requires: Option<&GatingRequirements>,
) -> Result<(), SkillRegistryError> {
    if let Some(requires) = openclaw_requires {
        let result = gating::check_requirements(requires).await;
        if !result.passed {
            return Err(SkillRegistryError::GatingFailed {
                name: name.to_string(),
                reason: result.failures.join("; "),
            });
        }
    }
    Ok(())
}

fn check_token_budget(
    name: &str,
    approx_tokens: usize,
    declared: usize,
) -> Result<(), SkillRegistryError> {
    if declared > 0 && approx_tokens > declared * 2 {
        return Err(SkillRegistryError::TokenBudgetExceeded {
            name: name.to_string(),
            approx_tokens,
            declared,
        });
    }
    Ok(())
}

/// Compute SHA-256 hash of content in the format "sha256:hex...".
pub fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    format!("sha256:{result:x}")
}

/// Helper to check gating for a `GatingRequirements`. Useful for callers that
/// don't have the full skill loaded yet.
pub async fn check_gating(
    requirements: &GatingRequirements,
) -> crate::skills::gating::GatingResult {
    gating::check_requirements(requirements).await
}
