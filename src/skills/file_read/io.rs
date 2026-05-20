//! Filesystem resolution and inline decoding for scoped skill file reads.

use std::path::{Path, PathBuf};

use tokio::io::AsyncReadExt;

use super::validation::{is_asset_path, path_not_readable};
use super::{
    MAX_SKILL_READ_FILE_BYTES, NON_INLINE_FETCH_HINT, SkillReadFileError, SkillReadFileErrorCode,
    SkillReadFileMetadata, SkillReadFileResponse, SkillReadFileSuccess,
};
use crate::skills::LoadedSkill;

struct ReadDisplay<'a> {
    skill: &'a str,
    path: String,
}

struct CanonicalTarget {
    path: PathBuf,
    size: u64,
}

pub(super) async fn read_validated_skill_file(
    skill: &LoadedSkill,
    relative_path: &Path,
) -> SkillReadFileResponse {
    let display = ReadDisplay {
        skill: skill.skill_identifier(),
        path: relative_path.to_string_lossy().replace('\\', "/"),
    };

    let target = match resolve_and_validate_target(skill.skill_root(), relative_path).await {
        Ok(target) => target,
        Err(error) => return error_response(&display, error),
    };
    let mime_type = mime_type_for(relative_path);
    let content = match read_inline_content(&target, relative_path, mime_type.clone()).await {
        Ok(content) => content,
        Err(error) => return error_response(&display, error),
    };

    SkillReadFileResponse::Success(SkillReadFileSuccess {
        skill: display.skill.to_string(),
        path: display.path,
        mime_type,
        content,
    })
}

async fn resolve_and_validate_target(
    root: &Path,
    relative_path: &Path,
) -> Result<CanonicalTarget, SkillReadFileError> {
    let canonical_root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| io_error("Skill root is not readable"))?;
    let target = root.join(relative_path);
    let metadata = readable_file_metadata(&target).await?;
    let canonical_target = tokio::fs::canonicalize(&target)
        .await
        .map_err(|_| io_error("File is not available for reading"))?;

    if !canonical_target.starts_with(&canonical_root) {
        return Err(path_not_readable());
    }

    Ok(CanonicalTarget {
        path: canonical_target,
        size: metadata.len(),
    })
}

async fn readable_file_metadata(path: &Path) -> Result<std::fs::Metadata, SkillReadFileError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(path_not_readable())
        }
        Ok(metadata) => Ok(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(path_not_readable()),
        Err(_) => Err(io_error("File is not available for reading")),
    }
}

async fn read_inline_content(
    target: &CanonicalTarget,
    relative_path: &Path,
    mime_type: String,
) -> Result<String, SkillReadFileError> {
    if target.size > MAX_SKILL_READ_FILE_BYTES {
        return Err(non_inline_error(
            SkillReadFileErrorCode::FileTooLarge,
            target.size,
            mime_type,
        ));
    }

    let bytes = read_file_contents(target).await?;
    parse_utf8_content(bytes, relative_path, target.size, mime_type)
}

async fn read_file_contents(target: &CanonicalTarget) -> Result<Vec<u8>, SkillReadFileError> {
    let mut file = open_readonly_no_follow(&target.path).await?;
    validate_opened_file(&file, target.size).await?;

    // The cast is safe because the size is first capped at
    // MAX_SKILL_READ_FILE_BYTES, which fits in usize on supported platforms.
    let mut contents = Vec::with_capacity(target.size.min(MAX_SKILL_READ_FILE_BYTES) as usize);
    file.read_to_end(&mut contents)
        .await
        .map_err(|_| io_error("File is not available for reading"))?;
    Ok(contents)
}

#[cfg(unix)]
async fn open_readonly_no_follow(path: &Path) -> Result<tokio::fs::File, SkillReadFileError> {
    tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .await
        .map_err(|error| match error.raw_os_error() {
            Some(libc::ELOOP) => path_not_readable(),
            _ => io_error("File is not available for reading"),
        })
}

#[cfg(not(unix))]
async fn open_readonly_no_follow(path: &Path) -> Result<tokio::fs::File, SkillReadFileError> {
    // Non-Unix platforms do not have the Unix O_NOFOLLOW path used above.
    // This fallback relies on the earlier symlink_metadata rejection and the
    // later validate_opened_file size check: file size must match stored size
    // after open or the open is rejected. TODO: use platform-specific atomic
    // no-follow open semantics for non-Unix targets when this tool supports
    // those platforms as first-class deployment environments.
    tokio::fs::File::open(path)
        .await
        .map_err(|_| io_error("File is not available for reading"))
}

async fn validate_opened_file(
    file: &tokio::fs::File,
    expected_size: u64,
) -> Result<(), SkillReadFileError> {
    let metadata = file
        .metadata()
        .await
        .map_err(|_| io_error("File is not available for reading"))?;
    if !metadata.is_file() {
        return Err(path_not_readable());
    }
    if metadata.len() != expected_size {
        return Err(io_error("File changed while reading"));
    }
    Ok(())
}

fn parse_utf8_content(
    bytes: Vec<u8>,
    relative_path: &Path,
    size: u64,
    mime_type: String,
) -> Result<String, SkillReadFileError> {
    match String::from_utf8(bytes) {
        Ok(content) => Ok(content),
        Err(_) if is_asset_path(relative_path) => Err(non_inline_error(
            SkillReadFileErrorCode::NonInlineAsset,
            size,
            mime_type,
        )),
        Err(_) => Err(SkillReadFileError::new(
            SkillReadFileErrorCode::InvalidUtf8,
            "File is not valid UTF-8 text",
        )),
    }
}

fn error_response(display: &ReadDisplay<'_>, error: SkillReadFileError) -> SkillReadFileResponse {
    SkillReadFileResponse::error(display.skill, &display.path, error)
}

fn io_error(message: impl Into<String>) -> SkillReadFileError {
    SkillReadFileError::new(SkillReadFileErrorCode::IoError, message)
}

fn non_inline_error(
    code: SkillReadFileErrorCode,
    size: u64,
    mime_type: String,
) -> SkillReadFileError {
    SkillReadFileError::with_metadata(
        code,
        "Phase 1 does not return binary or oversized assets inline.",
        metadata_payload(size, mime_type),
    )
}

fn metadata_payload(size: u64, mime_type: String) -> SkillReadFileMetadata {
    SkillReadFileMetadata {
        size,
        mime_type,
        fetch_hint: NON_INLINE_FETCH_HINT.to_string(),
    }
}

fn mime_type_for(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("text/plain")
        .to_string()
}
