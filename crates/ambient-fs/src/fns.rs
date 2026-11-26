//! Free functions mirroring the `std::fs` surface used by the host.
//!
//! Each function delegates directly to its `std::fs` counterpart; the crate
//! documentation explains why this ambient seam exists.

use std::io;
use std::path::{Path, PathBuf};

use crate::dir::ReadDir;
use crate::meta::{Metadata, Permissions};

/// Reads the entire contents of a file into a byte vector.
pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Reads the entire contents of a file into a string.
pub fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    std::fs::read_to_string(path)
}

/// Writes a slice as the entire contents of a file.
pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// Copies the contents of one file to another, returning the bytes copied.
pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<u64> {
    std::fs::copy(from, to)
}

/// Creates a new, empty directory at the provided path.
pub fn create_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// Recursively creates a directory and all missing parent components.
pub fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Removes an empty directory.
pub fn remove_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    std::fs::remove_dir(path)
}

/// Removes a directory after removing all of its contents.
pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Removes a file from the filesystem.
pub fn remove_file<P: AsRef<Path>>(path: P) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Renames a file or directory, replacing the destination if it exists.
pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Queries metadata for the given path, following symlinks.
pub fn metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    std::fs::metadata(path).map(Metadata::from_std)
}

/// Queries metadata for the given path without following a final symlink.
///
/// # Parameters
///
/// - `path`: the entry to inspect; if it is a symlink, the link itself is
///   described, so a dangling link still yields metadata.
///
/// # Returns
///
/// The entry's metadata.
///
/// # Errors
///
/// Fails when the entry does not exist or cannot be inspected.
pub fn symlink_metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    std::fs::symlink_metadata(path).map(Metadata::from_std)
}

/// Reads the target a symlink points at, without resolving it further.
///
/// # Parameters
///
/// - `path`: the symlink to read.
///
/// # Returns
///
/// The stored target exactly as written, which may be relative to the link's
/// parent directory and may contain `..`. It is not a resolved path, so a
/// containment check must resolve it component by component.
///
/// # Errors
///
/// Fails when `path` is not a symlink or cannot be read.
pub fn read_link<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    std::fs::read_link(path)
}

/// Sets the permissions of a file or directory.
pub fn set_permissions<P: AsRef<Path>>(path: P, perm: Permissions) -> io::Result<()> {
    std::fs::set_permissions(path, perm.into_std())
}

/// Returns an iterator over the entries within a directory.
pub fn read_dir<P: AsRef<Path>>(path: P) -> io::Result<ReadDir> {
    std::fs::read_dir(path).map(ReadDir::from_std)
}

/// Returns the canonical, absolute form of a path.
pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// Reports whether the path points at an existing entity.
pub fn try_exists<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    std::fs::exists(path)
}
