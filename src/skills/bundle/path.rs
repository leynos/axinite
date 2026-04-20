//! ZIP-entry path parsing for `.skill` bundle validation.
//!
//! This parser normalizes archive entry names into bundle-root and relative
//! path components while enforcing the RFC 0003 phase-1 shape constraints.
//! It rejects malformed or platform-specific paths early, including Windows
//! separators, absolute paths, empty segments, and traversal markers, so the
//! bundle validator can reason over a narrow, normalized path surface before
//! any archive contents are staged to disk.

use std::path::PathBuf;

use super::SkillBundleError;

#[derive(Debug)]
pub(super) struct ParsedBundlePath {
    root_name: String,
    relative_path: PathBuf,
    is_dir: bool,
}

impl ParsedBundlePath {
    pub(super) fn parse(raw_name: &str) -> Result<Self, SkillBundleError> {
        if is_malformed_raw_path(raw_name) {
            return Err(SkillBundleError::InvalidTopLevelPrefix);
        }

        let is_dir = raw_name.ends_with('/');
        let trimmed = raw_name.trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(SkillBundleError::InvalidTopLevelPrefix);
        }

        let segments: Vec<&str> = trimmed.split('/').collect();
        if segments.iter().any(|segment| segment.is_empty()) {
            return Err(SkillBundleError::InvalidTopLevelPrefix);
        }

        let root_name = segments[0].to_string();
        validate_root_name(&root_name)?;

        if segments.len() == 1 {
            if !is_dir {
                return Err(SkillBundleError::InvalidTopLevelPrefix);
            }
            return Ok(Self {
                root_name,
                relative_path: PathBuf::new(),
                is_dir,
            });
        }

        let relative_segments = &segments[1..];
        validate_relative_segments(raw_name, relative_segments, is_dir)?;

        Ok(Self {
            root_name,
            relative_path: relative_segments.iter().collect(),
            is_dir,
        })
    }

    pub(super) fn root_name(&self) -> &str {
        &self.root_name
    }

    pub(super) fn relative_path(&self) -> PathBuf {
        self.relative_path.clone()
    }

    pub(super) fn is_dir(&self) -> bool {
        self.is_dir
    }
}

fn validate_root_name(root_name: &str) -> Result<(), SkillBundleError> {
    let is_valid = !root_name.is_empty()
        && root_name.len() <= 64
        && root_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));

    if is_valid {
        Ok(())
    } else {
        Err(SkillBundleError::InvalidRootName {
            name: root_name.to_string(),
        })
    }
}

fn validate_relative_segments(
    raw_name: &str,
    segments: &[&str],
    is_dir: bool,
) -> Result<(), SkillBundleError> {
    if segments
        .iter()
        .any(|segment| matches!(*segment, "." | ".."))
    {
        return Err(SkillBundleError::InvalidTopLevelPrefix);
    }

    match segments {
        ["SKILL.md"] => Ok(()),
        _ if segments
            .iter()
            .skip(1)
            .any(|segment| *segment == "SKILL.md") =>
        {
            Err(SkillBundleError::NestedEntrypoint {
                path: raw_name.to_string(),
            })
        }
        ["scripts", ..] | ["bin", ..] => Err(SkillBundleError::DisallowedDirectory {
            path: raw_name.to_string(),
        }),
        ["references"] | ["assets"] if is_dir => Ok(()),
        ["references", _, ..] | ["assets", _, ..] => Ok(()),
        _ => Err(SkillBundleError::UnexpectedPath {
            path: raw_name.to_string(),
        }),
    }
}

/// Returns `true` when `raw_name` cannot possibly be a well-formed ZIP entry
/// path: it is empty, uses a Windows path separator, or is absolute.
fn is_malformed_raw_path(raw_name: &str) -> bool {
    raw_name.is_empty() || raw_name.contains('\\') || raw_name.starts_with('/')
}
