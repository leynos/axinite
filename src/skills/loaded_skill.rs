//! The [`LoadedSkill`] type: a fully parsed SKILL.md ready for
//! activation, together with its accessors and validation behaviour.

use super::*;

/// A fully loaded skill ready for activation.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    /// Parsed manifest from YAML frontmatter.
    pub manifest: SkillManifest,
    /// Raw prompt content (markdown body after frontmatter).
    pub prompt_content: String,
    /// Trust state (determined by source location).
    pub trust: SkillTrust,
    /// Where this skill was loaded from.
    pub source: SkillSource,
    /// Canonical runtime location and bundle layout metadata.
    location: LoadedSkillLocation,
    /// SHA-256 hash of the prompt content (computed at load time).
    pub content_hash: String,
    /// Pre-compiled regex patterns from activation criteria (compiled at load time).
    pub compiled_patterns: Vec<Regex>,
    /// Pre-computed lowercased keywords for scoring (avoids per-message allocation).
    /// Derived from `manifest.activation.keywords` at load time — do not mutate independently.
    pub lowercased_keywords: Vec<String>,
    /// Pre-computed lowercased exclude keywords for veto scoring.
    /// Derived from `manifest.activation.exclude_keywords` at load time.
    pub lowercased_exclude_keywords: Vec<String>,
    /// Pre-computed lowercased tags for scoring (avoids per-message allocation).
    /// Derived from `manifest.activation.tags` at load time — do not mutate independently.
    pub lowercased_tags: Vec<String>,
}

impl LoadedSkill {
    /// Create a loaded skill after validating location metadata.
    pub fn new(parts: LoadedSkillParts) -> Result<Self, LoadedSkillLocationError> {
        validate_location_matches_manifest(&parts.manifest, &parts.location)?;

        Ok(Self {
            manifest: parts.manifest,
            prompt_content: parts.prompt_content,
            trust: parts.trust,
            source: parts.source,
            location: parts.location,
            content_hash: parts.content_hash,
            compiled_patterns: parts.compiled_patterns,
            lowercased_keywords: parts.lowercased_keywords,
            lowercased_exclude_keywords: parts.lowercased_exclude_keywords,
            lowercased_tags: parts.lowercased_tags,
        })
    }

    /// Replace the location metadata after validating it against the manifest.
    pub fn set_location(
        &mut self,
        location: LoadedSkillLocation,
    ) -> Result<(), LoadedSkillLocationError> {
        validate_location_matches_manifest(&self.manifest, &location)?;
        self.location = location;
        Ok(())
    }

    /// Get the validated runtime location metadata.
    pub fn location(&self) -> &LoadedSkillLocation {
        &self.location
    }

    /// Get the skill name.
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Get the skill version.
    pub fn version(&self) -> &str {
        &self.manifest.version
    }

    /// Get the canonical skill identifier stored in runtime location metadata.
    pub fn skill_identifier(&self) -> &str {
        self.location.skill()
    }

    /// Get the private runtime root for scoped skill-file resolution.
    pub fn skill_root(&self) -> &std::path::Path {
        self.location.root()
    }

    /// Get the bundle-relative skill entrypoint.
    pub fn skill_entrypoint(&self) -> &std::path::Path {
        self.location.entrypoint()
    }

    /// Get whether this skill was loaded as a single file or bundle tree.
    pub fn package_kind(&self) -> SkillPackageKind {
        self.location.package_kind()
    }

    /// Compile regex patterns from activation criteria. Invalid or oversized patterns
    /// are logged and skipped. A size limit of 64 KiB is imposed on compiled regex
    /// state to prevent ReDoS via pathological patterns.
    pub fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
        /// Maximum compiled regex size (64 KiB) to prevent ReDoS.
        const MAX_REGEX_SIZE: usize = 1 << 16;

        patterns
            .iter()
            .filter_map(
                |p| match RegexBuilder::new(p).size_limit(MAX_REGEX_SIZE).build() {
                    Ok(re) => Some(re),
                    Err(e) => {
                        tracing::warn!("Invalid activation regex pattern '{}': {}", p, e);
                        None
                    }
                },
            )
            .collect()
    }
}

pub(crate) fn validate_location_matches_manifest(
    manifest: &SkillManifest,
    location: &LoadedSkillLocation,
) -> Result<(), LoadedSkillLocationError> {
    if manifest.name != location.skill() {
        return Err(LoadedSkillLocationError::new(format!(
            "skill location identifier '{}' does not match manifest name '{}'",
            location.skill(),
            manifest.name
        )));
    }

    // Defence-in-depth: `LoadedSkillLocation::new` already enforces
    // entrypoint relativity at construction, so a location that reaches
    // this point should always be bundle-relative.  The check remains
    // to guard against any future construction or deserialisation path
    // that might bypass `LoadedSkillLocation::new`.
    if !location.entrypoint().is_relative() {
        return Err(LoadedSkillLocationError::new(
            "skill entrypoint must be bundle-relative",
        ));
    }

    Ok(())
}
