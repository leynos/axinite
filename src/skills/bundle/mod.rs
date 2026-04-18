//! Validation for passive multi-file skill bundles.
//!
//! This module owns the `.skill` archive contract from RFC 0003. It validates
//! untrusted ZIP archives into a typed, already-decoded bundle view that the
//! registry can stage to disk without re-implementing archive policy in HTTP,
//! web, or tool adapters.

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use zip::read::ZipArchive;

use crate::skills::MAX_PROMPT_FILE_SIZE;

#[cfg(test)]
mod tests;

const ZIP_LOCAL_FILE_HEADER: &[u8; 4] = b"PK\x03\x04";
const ZIP_EMPTY_ARCHIVE: &[u8; 4] = b"PK\x05\x06";
const ZIP_SPANNED_ARCHIVE: &[u8; 4] = b"PK\x07\x08";

const MAX_BUNDLE_FILE_BYTES: u64 = 512 * 1024;
const MAX_BUNDLE_TOTAL_BYTES: u64 = 1024 * 1024;
const MAX_BUNDLE_FILE_COUNT: usize = 128;

const EXECUTABLE_EXTENSIONS: &[&str] = &["bat", "cmd", "js", "pl", "ps1", "py", "rb", "sh"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedBundleEntry {
    relative_path: PathBuf,
    contents: Vec<u8>,
}

impl ValidatedBundleEntry {
    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub(crate) fn contents(&self) -> &[u8] {
        &self.contents
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedSkillBundle {
    skill_name: String,
    entries: Vec<ValidatedBundleEntry>,
}

impl ValidatedSkillBundle {
    pub(crate) fn skill_name(&self) -> &str {
        &self.skill_name
    }

    pub(crate) fn entries(&self) -> &[ValidatedBundleEntry] {
        &self.entries
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SkillBundleError {
    #[error("invalid_skill_bundle: archive is not a valid ZIP file: {reason}")]
    InvalidArchive { reason: String },

    #[error(
        "invalid_skill_bundle: expected one top-level path prefix with SKILL.md at <root>/SKILL.md"
    )]
    InvalidTopLevelPrefix,

    #[error(
        "invalid_skill_bundle: bundle root '{name}' is invalid; use 1-64 ASCII letters, digits, '.', '_' or '-'"
    )]
    InvalidRootName { name: String },

    #[error("invalid_skill_bundle: missing <root>/SKILL.md")]
    MissingEntrypoint,

    #[error("invalid_skill_bundle: unexpected path '{path}'")]
    UnexpectedPath { path: String },

    #[error("invalid_skill_bundle: nested SKILL.md is not allowed at '{path}'")]
    NestedEntrypoint { path: String },

    #[error("invalid_skill_bundle: directory '{path}' is not allowed in phase 1")]
    DisallowedDirectory { path: String },

    #[error("invalid_skill_bundle: executable payloads are not allowed: '{path}'")]
    ExecutablePayload { path: String },

    #[error("invalid_skill_bundle: archive entry '{path}' uses an unsupported file type")]
    UnsupportedFileType { path: String },

    #[error("invalid_skill_bundle: duplicate path after normalization: '{path}'")]
    DuplicatePath { path: String },

    #[error("invalid_skill_bundle: file '{path}' is {size} bytes (max {max} bytes)")]
    EntryTooLarge { path: String, size: u64, max: u64 },

    #[error("invalid_skill_bundle: archive totals {size} bytes (max {max} bytes)")]
    ArchiveTooLarge { size: u64, max: u64 },

    #[error("invalid_skill_bundle: archive contains {count} files (max {max} files)")]
    TooManyFiles { count: usize, max: usize },

    #[error("invalid_skill_bundle: text file '{path}' is not valid UTF-8")]
    InvalidUtf8Text { path: String },

    #[error("invalid_skill_bundle: failed to read '{path}' from archive: {reason}")]
    ReadFailure { path: String, reason: String },
}

pub(crate) fn looks_like_skill_archive(bytes: &[u8]) -> bool {
    bytes.starts_with(ZIP_LOCAL_FILE_HEADER)
        || bytes.starts_with(ZIP_EMPTY_ARCHIVE)
        || bytes.starts_with(ZIP_SPANNED_ARCHIVE)
}

pub(crate) fn validate_skill_archive(
    archive_bytes: &[u8],
) -> Result<ValidatedSkillBundle, SkillBundleError> {
    let cursor = Cursor::new(archive_bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|error| SkillBundleError::InvalidArchive {
            reason: error.to_string(),
        })?;

    let mut root_name: Option<String> = None;
    let mut found_skill_md = false;
    let mut total_bytes = 0u64;
    let mut file_count = 0usize;
    let mut seen_paths = HashSet::new();
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let mut file =
            archive
                .by_index(index)
                .map_err(|error| SkillBundleError::InvalidArchive {
                    reason: error.to_string(),
                })?;
        let raw_name = file.name().to_string();
        let entry = ParsedBundlePath::parse(&raw_name)?;

        match &root_name {
            Some(expected) if expected != entry.root_name() => {
                return Err(SkillBundleError::InvalidTopLevelPrefix);
            }
            None => root_name = Some(entry.root_name().to_string()),
            Some(_) => {}
        }

        validate_file_type(&raw_name, file.unix_mode(), entry.is_dir())?;
        if entry.is_dir() {
            continue;
        }

        file_count += 1;
        if file_count > MAX_BUNDLE_FILE_COUNT {
            return Err(SkillBundleError::TooManyFiles {
                count: file_count,
                max: MAX_BUNDLE_FILE_COUNT,
            });
        }

        let relative_path = entry.relative_path();
        let normalized_path = relative_path.to_string_lossy().to_lowercase();
        if !seen_paths.insert(normalized_path) {
            return Err(SkillBundleError::DuplicatePath {
                path: relative_path.display().to_string(),
            });
        }

        let max_size = if relative_path == Path::new("SKILL.md") {
            MAX_PROMPT_FILE_SIZE
        } else {
            MAX_BUNDLE_FILE_BYTES
        };
        let size = file.size();
        if size > max_size {
            return Err(SkillBundleError::EntryTooLarge {
                path: relative_path.display().to_string(),
                size,
                max: max_size,
            });
        }

        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_BUNDLE_TOTAL_BYTES {
            return Err(SkillBundleError::ArchiveTooLarge {
                size: total_bytes,
                max: MAX_BUNDLE_TOTAL_BYTES,
            });
        }

        let mut contents = Vec::with_capacity(size as usize);
        file.read_to_end(&mut contents)
            .map_err(|error| SkillBundleError::ReadFailure {
                path: raw_name.clone(),
                reason: error.to_string(),
            })?;

        if relative_path == Path::new("SKILL.md") {
            found_skill_md = true;
            std::str::from_utf8(&contents).map_err(|_| SkillBundleError::InvalidUtf8Text {
                path: raw_name.clone(),
            })?;
        } else if is_reference_file(&relative_path) {
            std::str::from_utf8(&contents).map_err(|_| SkillBundleError::InvalidUtf8Text {
                path: raw_name.clone(),
            })?;
        }

        entries.push(ValidatedBundleEntry {
            relative_path,
            contents,
        });
    }

    let skill_name = root_name.ok_or(SkillBundleError::MissingEntrypoint)?;
    if !found_skill_md {
        return Err(SkillBundleError::MissingEntrypoint);
    }

    Ok(ValidatedSkillBundle {
        skill_name,
        entries,
    })
}

fn validate_file_type(
    path: &str,
    unix_mode: Option<u32>,
    is_dir: bool,
) -> Result<(), SkillBundleError> {
    if let Some(mode) = unix_mode {
        let file_type = mode & 0o170000;
        let is_regular = file_type == 0 || file_type == 0o100000;
        let is_directory = file_type == 0o040000;
        if !(is_regular || is_directory) {
            return Err(SkillBundleError::UnsupportedFileType {
                path: path.to_string(),
            });
        }
        if !is_dir && mode & 0o111 != 0 {
            return Err(SkillBundleError::ExecutablePayload {
                path: path.to_string(),
            });
        }
    }

    if !is_dir && has_executable_extension(path) {
        return Err(SkillBundleError::ExecutablePayload {
            path: path.to_string(),
        });
    }

    Ok(())
}

fn has_executable_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    Path::new(&lower)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXECUTABLE_EXTENSIONS.contains(&ext))
}

fn is_reference_file(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|component| component.as_os_str() == "references")
}

#[derive(Debug)]
struct ParsedBundlePath {
    root_name: String,
    relative_path: PathBuf,
    is_dir: bool,
}

impl ParsedBundlePath {
    fn parse(raw_name: &str) -> Result<Self, SkillBundleError> {
        if raw_name.is_empty() || raw_name.contains('\\') || raw_name.starts_with('/') {
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

    fn root_name(&self) -> &str {
        &self.root_name
    }

    fn relative_path(&self) -> PathBuf {
        self.relative_path.clone()
    }

    fn is_dir(&self) -> bool {
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
        ["references", ..] | ["assets", ..] => Ok(()),
        _ => Err(SkillBundleError::UnexpectedPath {
            path: raw_name.to_string(),
        }),
    }
}
