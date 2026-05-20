//! Bundle-relative path validation for scoped skill file reads.

use std::path::{Component, Path, PathBuf};

use super::{SkillReadFileError, SkillReadFileErrorCode};

pub(super) fn display_path(requested_path: &str) -> String {
    requested_path.replace('\\', "/")
}

pub(super) fn validate_bundle_relative_path(
    requested_path: &str,
) -> Result<PathBuf, SkillReadFileError> {
    if requested_path.trim().is_empty() || requested_path.contains('\\') {
        return Err(path_not_readable());
    }

    let path = Path::new(requested_path);
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => segments.push(segment.to_owned()),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(path_not_readable());
            }
        }
    }

    if !is_allowed_bundle_path(&segments) {
        return Err(path_not_readable());
    }

    Ok(segments.iter().collect())
}

fn is_allowed_bundle_path(segments: &[std::ffi::OsString]) -> bool {
    match segments {
        [entrypoint] => entrypoint == "SKILL.md",
        [root, rest @ ..] => {
            (root == "references" || root == "assets")
                && !rest.is_empty()
                && rest.iter().all(|segment| segment != "SKILL.md")
        }
        _ => false,
    }
}

pub(super) fn path_not_readable() -> SkillReadFileError {
    SkillReadFileError::new(
        SkillReadFileErrorCode::PathNotReadable,
        "File is not available for reading",
    )
}

pub(super) fn is_asset_path(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|component| component.as_os_str() == "assets")
}
