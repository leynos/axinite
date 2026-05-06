//! OpenClaw SKILL.md-based skills system for IronClaw.
//!
//! Skills are SKILL.md files (YAML frontmatter + markdown prompt) that extend the
//! agent's behavior through prompt-level instructions. Unlike code-level tools
//! (WASM/MCP), skills operate in the LLM context and are subject to trust-based
//! authority attenuation.
//!
//! # Trust Model
//!
//! Skills have two trust states that determine their authority:
//! - **Trusted**: User-placed skills (local/workspace) with full tool access
//! - **Installed**: Registry/external skills, restricted to read-only tools
//!
//! The effective tool ceiling is determined by the *lowest-trust* active skill,
//! preventing privilege escalation through skill mixing.

pub mod attenuation;
pub mod bundle;
pub mod catalog;
pub mod escape;
pub mod gating;
/// Shared source-field normalisation helpers for skill install adapters.
pub(crate) mod install_source;
pub mod parser;
pub mod registry;
pub mod selector;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub use attenuation::{AttenuationResult, attenuate_tools};
pub use escape::{escape_skill_content, escape_xml_attr, normalize_line_endings};
pub use registry::SkillRegistry;
pub use selector::prefilter_skills;

use std::path::PathBuf;

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

/// Maximum number of keywords allowed per skill to prevent scoring manipulation.
const MAX_KEYWORDS_PER_SKILL: usize = 20;

/// Maximum number of regex patterns allowed per skill.
const MAX_PATTERNS_PER_SKILL: usize = 5;

/// Maximum number of tags allowed per skill to prevent scoring manipulation.
const MAX_TAGS_PER_SKILL: usize = 10;

/// Minimum length for keywords and tags. Short tokens like "a" or "is"
/// match too broadly and can be used to game the scoring system.
const MIN_KEYWORD_TAG_LENGTH: usize = 3;

/// Maximum file size for SKILL.md (64 KiB).
pub const MAX_PROMPT_FILE_SIZE: u64 = 64 * 1024;

/// Regex for validating skill names: alphanumeric, hyphens, underscores, dots.
static SKILL_NAME_PATTERN: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,63}$").unwrap());

/// Validate a skill name against the allowed pattern.
pub fn validate_skill_name(name: &str) -> bool {
    SKILL_NAME_PATTERN.is_match(name)
}

/// Trust state for a skill, determining its authority ceiling.
///
/// SAFETY: Variant ordering matters. `Ord` is derived from discriminant values
/// and the security model relies on `Installed < Trusted`. Do NOT reorder
/// variants or change discriminant values without auditing all `min()` /
/// comparison call-sites in attenuation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrust {
    /// Registry/external skill. Read-only tools only.
    Installed = 0,
    /// User-placed skill (local or workspace). Full trust, all tools available.
    Trusted = 1,
}

impl std::fmt::Display for SkillTrust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Installed => write!(f, "installed"),
            Self::Trusted => write!(f, "trusted"),
        }
    }
}

/// Where a skill was loaded from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Workspace skills directory (<workspace>/skills/).
    Workspace(PathBuf),
    /// User skills directory (~/.ironclaw/skills/).
    User(PathBuf),
    /// Bundled with the application.
    Bundled(PathBuf),
}

/// How a skill's installed files are laid out on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillPackageKind {
    /// A lone `SKILL.md` file with no bundle-only support files.
    SingleFile,
    /// A bundle tree containing `SKILL.md` plus bundle-relative support files.
    Bundle,
}

impl SkillPackageKind {
    /// Stable prompt-facing label for the package kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleFile => "single_file",
            Self::Bundle => "bundle",
        }
    }
}

/// Canonical runtime location metadata for a loaded skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSkillLocation {
    skill: String,
    root: PathBuf,
    entrypoint: PathBuf,
    package_kind: SkillPackageKind,
}

/// Error returned when loaded skill location metadata is inconsistent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSkillLocationError {
    reason: String,
}

impl LoadedSkillLocationError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for LoadedSkillLocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for LoadedSkillLocationError {}

impl LoadedSkillLocation {
    /// Create location metadata for a loaded skill.
    ///
    /// `root` is the private runtime filesystem root used by future scoped
    /// skill-file reads. `entrypoint` is bundle-relative and must not be an
    /// absolute host path.
    ///
    /// # Errors
    ///
    /// Returns [`LoadedSkillLocationError`] if `entrypoint` is an absolute
    /// host path rather than a bundle-relative path.
    pub fn new(
        skill: impl Into<String>,
        root: impl Into<PathBuf>,
        entrypoint: impl Into<PathBuf>,
        package_kind: SkillPackageKind,
    ) -> Result<Self, LoadedSkillLocationError> {
        let entrypoint = entrypoint.into();
        if !entrypoint.is_relative() {
            return Err(LoadedSkillLocationError::new(
                "skill entrypoint must be bundle-relative",
            ));
        }

        Ok(Self {
            skill: skill.into(),
            root: root.into(),
            entrypoint,
            package_kind,
        })
    }

    /// Canonical skill identifier exposed to model-facing skill context.
    pub fn skill(&self) -> &str {
        &self.skill
    }

    /// Private canonical filesystem root for runtime file resolution.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// Stable bundle-relative root exposed in model-facing metadata.
    pub fn bundle_relative_root(&self) -> &std::path::Path {
        std::path::Path::new(".")
    }

    /// Bundle-relative entrypoint, normally `SKILL.md`.
    pub fn entrypoint(&self) -> &std::path::Path {
        &self.entrypoint
    }

    /// Package mode used to distinguish single-file skills from bundles.
    pub fn package_kind(&self) -> SkillPackageKind {
        self.package_kind
    }
}

/// Construction payload for a fully loaded skill.
pub struct LoadedSkillParts {
    /// Parsed manifest from YAML frontmatter.
    pub manifest: SkillManifest,
    /// Raw prompt content (markdown body after frontmatter).
    pub prompt_content: String,
    /// Trust state (determined by source location).
    pub trust: SkillTrust,
    /// Where this skill was loaded from.
    pub source: SkillSource,
    /// Canonical runtime location and bundle layout metadata.
    pub location: LoadedSkillLocation,
    /// SHA-256 hash of the prompt content (computed at load time).
    pub content_hash: String,
    /// Pre-compiled regex patterns from activation criteria (compiled at load time).
    pub compiled_patterns: Vec<Regex>,
    /// Pre-computed lowercased keywords for scoring.
    pub lowercased_keywords: Vec<String>,
    /// Pre-computed lowercased exclude keywords for veto scoring.
    pub lowercased_exclude_keywords: Vec<String>,
    /// Pre-computed lowercased tags for scoring.
    pub lowercased_tags: Vec<String>,
}

/// Activation criteria parsed from SKILL.md frontmatter `activation` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivationCriteria {
    /// Keywords that trigger this skill (exact and substring match).
    /// Capped at `MAX_KEYWORDS_PER_SKILL` during loading.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Keywords that veto this skill — if any match, score is 0 regardless of
    /// keyword/pattern matches. Prevents cross-skill interference.
    #[serde(default)]
    pub exclude_keywords: Vec<String>,
    /// Regex patterns for more complex matching.
    /// Capped at `MAX_PATTERNS_PER_SKILL` during loading.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Tags for broad category matching.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Maximum context tokens this skill's prompt should consume.
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
}

impl ActivationCriteria {
    /// Enforce limits on keywords, patterns, and tags to prevent scoring manipulation.
    ///
    /// Filters out short keywords/tags (< 3 chars) that match too broadly,
    /// then truncates to per-field caps.
    pub fn enforce_limits(&mut self) {
        self.keywords.retain(|k| k.len() >= MIN_KEYWORD_TAG_LENGTH);
        self.keywords.truncate(MAX_KEYWORDS_PER_SKILL);
        self.exclude_keywords
            .retain(|k| k.len() >= MIN_KEYWORD_TAG_LENGTH);
        self.exclude_keywords.truncate(MAX_KEYWORDS_PER_SKILL);
        self.patterns.truncate(MAX_PATTERNS_PER_SKILL);
        self.tags.retain(|t| t.len() >= MIN_KEYWORD_TAG_LENGTH);
        self.tags.truncate(MAX_TAGS_PER_SKILL);
    }
}

fn default_max_context_tokens() -> usize {
    2000
}

/// Parsed skill manifest from SKILL.md YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    /// Skill name (validated against SKILL_NAME_PATTERN).
    pub name: String,
    /// Skill version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Short description of the skill.
    #[serde(default)]
    pub description: String,
    /// Activation criteria.
    #[serde(default)]
    pub activation: ActivationCriteria,
    /// Optional OpenClaw metadata.
    #[serde(default)]
    pub metadata: Option<SkillMetadata>,
}

fn default_version() -> String {
    "0.0.0".to_string()
}

/// Optional metadata section in SKILL.md frontmatter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// OpenClaw-specific metadata.
    #[serde(default)]
    pub openclaw: Option<OpenClawMeta>,
}

/// OpenClaw-specific metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenClawMeta {
    /// Gating requirements that must be met for the skill to load.
    #[serde(default)]
    pub requires: GatingRequirements,
}

/// Requirements that must be satisfied for a skill to load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatingRequirements {
    /// Required binaries that must be on PATH.
    #[serde(default)]
    pub bins: Vec<String>,
    /// Required environment variables that must be set.
    #[serde(default)]
    pub env: Vec<String>,
    /// Required config file paths that must exist.
    #[serde(default)]
    pub config: Vec<String>,
}

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
