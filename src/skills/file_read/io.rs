//! Filesystem resolution and inline decoding for scoped skill file reads.

use std::path::Path;

use tokio::io::{AsyncReadExt, Take};

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

struct OpenedTarget {
    file: tokio::fs::File,
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

    let target = match open_validated_target(skill.skill_root(), relative_path).await {
        Ok(target) => target,
        Err(error) => return error_response(&display, error),
    };
    let mime_type = mime_type_for(relative_path);
    let content = match read_inline_content(target, relative_path, mime_type.clone()).await {
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

async fn open_validated_target(
    root: &Path,
    relative_path: &Path,
) -> Result<OpenedTarget, SkillReadFileError> {
    open_relative_to_root(root, relative_path).await
}

async fn read_inline_content(
    target: OpenedTarget,
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

    let size = target.size;
    let bytes = read_file_contents(target).await?;
    let actual_size = bytes.len() as u64;
    if actual_size > MAX_SKILL_READ_FILE_BYTES {
        return Err(non_inline_error(
            SkillReadFileErrorCode::FileTooLarge,
            actual_size,
            mime_type,
        ));
    }
    parse_utf8_content(bytes, relative_path, size, mime_type)
}

async fn read_file_contents(target: OpenedTarget) -> Result<Vec<u8>, SkillReadFileError> {
    let OpenedTarget { mut file, size } = target;
    // The cast is safe because the size is first capped at the read limit,
    // which fits in usize on supported platforms.
    let read_limit = MAX_SKILL_READ_FILE_BYTES + 1;
    let mut contents = Vec::with_capacity(size.min(read_limit) as usize);
    limited_reader(&mut file, read_limit)
        .read_to_end(&mut contents)
        .await
        .map_err(|_| io_error("File is not available for reading"))?;
    Ok(contents)
}

fn limited_reader(file: &mut tokio::fs::File, limit: u64) -> Take<&mut tokio::fs::File> {
    file.take(limit)
}

async fn metadata_for_opened_file(file: &tokio::fs::File) -> Result<u64, SkillReadFileError> {
    let metadata = file
        .metadata()
        .await
        .map_err(|_| io_error("File is not available for reading"))?;
    if !metadata.is_file() {
        return Err(path_not_readable());
    }
    Ok(metadata.len())
}

#[cfg(target_os = "linux")]
async fn open_relative_to_root(
    root: &Path,
    relative_path: &Path,
) -> Result<OpenedTarget, SkillReadFileError> {
    use std::ffi::CString;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;

    let root = open_root_directory(root)?;
    if !root
        .metadata()
        .map_err(|_| io_error("Skill root is not readable"))?
        .is_dir()
    {
        return Err(path_not_readable());
    }

    let path =
        CString::new(relative_path.as_os_str().as_bytes()).map_err(|_| path_not_readable())?;
    // SAFETY: Linux requires `open_how` to be zero-initialized so unknown
    // future fields default to zero before the supported fields are set.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_RDONLY | libc::O_CLOEXEC) as u64;
    how.mode = 0;
    how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS;

    // SAFETY: `root` is a live directory file descriptor, `path` is a
    // NUL-terminated relative path owned by this stack frame, and `how` points
    // to an initialized `open_how` with its correct size. `openat2` returns a
    // new file descriptor on success, which is immediately wrapped in
    // `OwnedFd` to transfer ownership and ensure it is closed.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            root.as_raw_fd(),
            path.as_ptr(),
            &how,
            std::mem::size_of::<libc::open_how>(),
        )
    };

    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return Err(openat2_error(error));
    }

    // SAFETY: `fd` was returned by a successful `openat2` call above and has
    // not been transferred elsewhere.
    let file = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) });
    let file = tokio::fs::File::from_std(file);
    let size = metadata_for_opened_file(&file).await?;

    Ok(OpenedTarget { file, size })
}

#[cfg(target_os = "linux")]
fn open_root_directory(root: &Path) -> Result<std::fs::File, SkillReadFileError> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(root)
        .map_err(|_| io_error("Skill root is not readable"))
}

#[cfg(not(target_os = "linux"))]
async fn open_relative_to_root(
    _root: &Path,
    _relative_path: &Path,
) -> Result<OpenedTarget, SkillReadFileError> {
    tracing::warn!(
        "skill_read_file is unavailable on this platform because atomic symlink-safe reads require Linux openat2"
    );
    Err(io_error(
        "Atomic skill file reads are not supported on this platform",
    ))
}

#[cfg(target_os = "linux")]
fn openat2_error(error: std::io::Error) -> SkillReadFileError {
    match error.raw_os_error() {
        Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP | libc::EXDEV | libc::EAGAIN) => {
            path_not_readable()
        }
        Some(libc::ENOSYS | libc::EINVAL) => io_error("Atomic skill file reads are not supported"),
        _ => io_error("File is not available for reading"),
    }
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
