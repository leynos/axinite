//! Errors raised by skill registry operations.

use crate::skills::bundle::SkillBundleError;

/// Error type for skill registry operations.
#[derive(Debug, thiserror::Error)]
pub enum SkillRegistryError {
    /// No skill with the given name is registered.
    #[error("Skill not found: {0}")]
    NotFound(String),

    /// A skill file exists but could not be read from disk.
    #[error("Failed to read skill file {path}: {reason}")]
    ReadError {
        /// Path of the file that could not be read.
        path: String,
        /// I/O error message.
        reason: String,
    },

    /// A SKILL.md file could not be parsed.
    #[error("Failed to parse SKILL.md for '{name}': {reason}")]
    ParseError {
        /// Name of the skill whose file failed to parse.
        name: String,
        /// Parser error message.
        reason: String,
    },

    /// A skill file is larger than the permitted size.
    #[error("Skill file too large for '{name}': {size} bytes (max {max} bytes)")]
    FileTooLarge {
        /// Name of the oversized skill.
        name: String,
        /// Size of the file in bytes.
        size: u64,
        /// Largest permitted file size in bytes.
        max: u64,
    },

    /// A symlink was found in a skills directory, which is refused.
    #[error("Symlink detected in skills directory: {path}")]
    SymlinkDetected {
        /// Path of the symlink.
        path: String,
    },

    /// A skill's gating requirements are not met, so it cannot be loaded.
    #[error("Skill '{name}' failed gating: {reason}")]
    GatingFailed {
        /// Name of the skill that was gated out.
        name: String,
        /// Requirement that was not satisfied.
        reason: String,
    },

    /// The skill prompt is estimated to exceed its declared token budget.
    #[error(
        "Skill '{name}' prompt exceeds token budget: ~{approx_tokens} tokens but declares max_context_tokens={declared}"
    )]
    TokenBudgetExceeded {
        /// Name of the skill.
        name: String,
        /// Estimated token count of the prompt.
        approx_tokens: usize,
        /// Budget declared in `max_context_tokens`.
        declared: usize,
    },

    /// A skill with this name is already installed.
    #[error("Skill '{name}' already exists")]
    AlreadyExists {
        /// Name of the existing skill.
        name: String,
    },

    /// The skill's source does not allow removal through the registry, such as workspace or
    /// bundled skills.
    #[error("Cannot remove skill '{name}': {reason}")]
    CannotRemove {
        /// Name of the skill that was to be removed.
        name: String,
        /// Why removal is refused.
        reason: String,
    },

    /// A skill file could not be written to disk.
    #[error("Failed to write skill file {path}: {reason}")]
    WriteError {
        /// Path of the file that could not be written.
        path: String,
        /// I/O error message.
        reason: String,
    },

    /// A skill bundle failed validation; its message is shown unchanged.
    #[error("{0}")]
    InvalidBundle(#[from] SkillBundleError),

    /// Skill content or its location metadata failed validation, such as non-UTF-8 text.
    #[error("Invalid skill content: {reason}")]
    InvalidContent {
        /// Description of what is wrong with the content.
        reason: String,
    },
}

impl From<crate::skills::LoadedSkillLocationError> for SkillRegistryError {
    fn from(error: crate::skills::LoadedSkillLocationError) -> Self {
        SkillRegistryError::InvalidContent {
            reason: error.to_string(),
        }
    }
}
