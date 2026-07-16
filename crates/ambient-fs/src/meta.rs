//! Wrappers around `std::fs` metadata types.
//!
//! The Whitaker `no_std_fs_operations` lint resolves receiver types, so
//! re-exports would still be flagged at call sites; these newtypes delegate
//! instead.

use std::time::SystemTime;

/// Metadata information about a file, wrapping [`std::fs::Metadata`].
#[derive(Clone, Debug)]
pub struct Metadata(std::fs::Metadata);

impl Metadata {
    /// Wraps a [`std::fs::Metadata`] value.
    #[must_use]
    pub fn from_std(inner: std::fs::Metadata) -> Self {
        Self(inner)
    }

    /// Reports whether this metadata describes a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.0.is_dir()
    }

    /// Reports whether this metadata describes a regular file.
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }

    /// Reports whether this metadata describes a symbolic link.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        self.0.is_symlink()
    }

    /// Returns the size of the file in bytes.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.0.len()
    }

    /// Reports whether the file is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    /// Returns the file type for this metadata.
    #[must_use]
    pub fn file_type(&self) -> FileType {
        FileType(self.0.file_type())
    }

    /// Returns the permissions of the file.
    #[must_use]
    pub fn permissions(&self) -> Permissions {
        Permissions(self.0.permissions())
    }

    /// Returns the last modification time, when available.
    pub fn modified(&self) -> std::io::Result<SystemTime> {
        self.0.modified()
    }

    /// Returns the creation time, when available.
    pub fn created(&self) -> std::io::Result<SystemTime> {
        self.0.created()
    }
}

/// Permissions of a file, wrapping [`std::fs::Permissions`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permissions(std::fs::Permissions);

impl Permissions {
    /// Wraps a [`std::fs::Permissions`] value.
    #[must_use]
    pub fn from_std(inner: std::fs::Permissions) -> Self {
        Self(inner)
    }

    /// Unwraps into the underlying [`std::fs::Permissions`].
    #[must_use]
    pub fn into_std(self) -> std::fs::Permissions {
        self.0
    }

    /// Reports whether the readonly flag is set.
    #[must_use]
    pub fn readonly(&self) -> bool {
        self.0.readonly()
    }

    /// Sets or clears the readonly flag.
    pub fn set_readonly(&mut self, readonly: bool) {
        self.0.set_readonly(readonly);
    }

    /// Creates permissions from Unix mode bits.
    #[cfg(unix)]
    #[must_use]
    pub fn from_mode(mode: u32) -> Self {
        use std::os::unix::fs::PermissionsExt;
        Self(std::fs::Permissions::from_mode(mode))
    }

    /// Returns the Unix mode bits.
    #[cfg(unix)]
    #[must_use]
    pub fn mode(&self) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        self.0.mode()
    }

    /// Sets the Unix mode bits.
    #[cfg(unix)]
    pub fn set_mode(&mut self, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        self.0.set_mode(mode);
    }
}

/// The type of a directory entry, wrapping [`std::fs::FileType`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileType(std::fs::FileType);

impl FileType {
    /// Wraps a [`std::fs::FileType`] value.
    #[must_use]
    pub fn from_std(inner: std::fs::FileType) -> Self {
        Self(inner)
    }

    /// Reports whether this type describes a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.0.is_dir()
    }

    /// Reports whether this type describes a regular file.
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }

    /// Reports whether this type describes a symbolic link.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        self.0.is_symlink()
    }
}
