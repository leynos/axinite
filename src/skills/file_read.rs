//! Read-only, skill-scoped access to bundled skill files.
//!
//! This module owns the runtime policy for `skill_read_file`: callers provide a
//! loaded skill and a bundle-relative path, and the policy either returns UTF-8
//! text inline or a deterministic model-facing denial payload.

use serde::Serialize;

use crate::skills::LoadedSkill;

mod io;
mod validation;

use io::read_validated_skill_file;
use validation::{display_path, validate_bundle_relative_path};

/// Maximum file size, in bytes, returned inline by `skill_read_file`.
pub const MAX_SKILL_READ_FILE_BYTES: u64 = crate::skills::MAX_PROMPT_FILE_SIZE;

const NON_INLINE_FETCH_HINT: &str =
    "Treat this as a passive asset; request only referenced text files in phase 1.";

/// Complete model-facing response for a skill file read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SkillReadFileResponse {
    /// UTF-8 text returned inline.
    Success(SkillReadFileSuccess),
    /// Expected skill-scoped denial or read failure.
    Error(SkillReadFileErrorResponse),
}

impl SkillReadFileResponse {
    /// Deterministic error payload for a skill that is not loaded.
    pub fn unknown_skill(skill: &str, path: &str) -> Self {
        Self::error(
            skill,
            path,
            SkillReadFileError::new(
                SkillReadFileErrorCode::UnknownSkill,
                "Skill is not loaded or available for reading",
            ),
        )
    }

    fn error(skill: &str, path: &str, error: SkillReadFileError) -> Self {
        Self::Error(SkillReadFileErrorResponse {
            skill: skill.to_string(),
            path: path.to_string(),
            error,
        })
    }
}

/// Successful inline text response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillReadFileSuccess {
    /// Canonical skill identifier used for the read.
    pub skill: String,
    /// Normalized bundle-relative path that was read.
    pub path: String,
    /// Best-effort media type inferred from the bundle-relative path.
    pub mime_type: String,
    /// UTF-8 file content returned inline.
    pub content: String,
}

/// Expected error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillReadFileErrorResponse {
    /// Canonical skill identifier, or the requested identifier for unknown skills.
    pub skill: String,
    /// Requested or normalized bundle-relative path.
    pub path: String,
    /// Stable, model-facing error payload.
    pub error: SkillReadFileError,
}

/// Stable error payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillReadFileError {
    /// Machine-readable reason for the denial or failure.
    pub code: SkillReadFileErrorCode,
    /// Short human-readable explanation for the model.
    pub message: String,
    /// Size, media type, and fetch guidance for non-inline files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SkillReadFileMetadata>,
}

impl SkillReadFileError {
    fn new(code: SkillReadFileErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            metadata: None,
        }
    }

    fn with_metadata(
        code: SkillReadFileErrorCode,
        message: impl Into<String>,
        metadata: SkillReadFileMetadata,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            metadata: Some(metadata),
        }
    }
}

/// Stable model-facing error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillReadFileErrorCode {
    /// The requested skill is not currently loaded.
    UnknownSkill,
    /// The requested path is missing, disallowed, non-regular, or outside the skill root.
    PathNotReadable,
    /// The requested asset is binary and cannot be returned inline in phase 1.
    NonInlineAsset,
    /// The requested file is larger than [`MAX_SKILL_READ_FILE_BYTES`].
    FileTooLarge,
    /// The requested non-asset file is not valid UTF-8.
    InvalidUtf8,
    /// Filesystem I/O failed while resolving or reading the scoped file.
    IoError,
}

/// Metadata for files that cannot be returned inline in phase 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillReadFileMetadata {
    /// File size in bytes.
    pub size: u64,
    /// Best-effort media type inferred from the bundle-relative path.
    pub mime_type: String,
    /// Stable instruction for how the model should treat non-inline content.
    pub fetch_hint: String,
}

/// Read a bundle-relative file under the loaded skill's private root.
///
/// The caller supplies an already loaded skill and a path such as `SKILL.md`,
/// `references/usage.md`, or `assets/note.txt`. The path is validated before
/// any filesystem access: absolute paths, traversal, backslashes, unsupported
/// roots, and nested `SKILL.md` files are rejected. Successful reads return
/// inline UTF-8 text; expected denials return a typed
/// [`SkillReadFileResponse::Error`] payload instead of leaking host paths.
///
/// # Example
///
/// ```no_run
/// # async fn example(skill: &ironclaw::skills::LoadedSkill) {
/// use ironclaw::skills::file_read::{SkillReadFileResponse, read_skill_file};
///
/// match read_skill_file(skill, "references/usage.md").await {
///     SkillReadFileResponse::Success(file) => {
///         println!("{}", file.content);
///     }
///     SkillReadFileResponse::Error(error) => {
///         eprintln!("{}: {}", error.path, error.error.message);
///     }
/// }
/// # }
/// ```
pub async fn read_skill_file(skill: &LoadedSkill, requested_path: &str) -> SkillReadFileResponse {
    let display_skill = skill.skill_identifier();
    let display_path = display_path(requested_path);
    let relative_path = match validate_bundle_relative_path(requested_path) {
        Ok(path) => path,
        Err(error) => return SkillReadFileResponse::error(display_skill, &display_path, error),
    };

    read_validated_skill_file(skill, &relative_path).await
}

#[cfg(test)]
mod tests;
