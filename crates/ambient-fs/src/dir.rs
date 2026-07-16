//! Directory-iteration wrappers around `std::fs`.
//!
//! [`ReadDir`] and [`DirEntry`] delegate to their `std::fs` counterparts so
//! that directory walks stay inside this crate's ambient seam.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use crate::meta::{FileType, Metadata};

/// Iterator over directory entries, wrapping [`std::fs::ReadDir`].
#[derive(Debug)]
pub struct ReadDir(std::fs::ReadDir);

impl ReadDir {
    /// Wraps a [`std::fs::ReadDir`] iterator.
    #[must_use]
    pub fn from_std(inner: std::fs::ReadDir) -> Self {
        Self(inner)
    }
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|entry| entry.map(DirEntry))
    }
}

/// A directory entry, wrapping [`std::fs::DirEntry`].
#[derive(Debug)]
pub struct DirEntry(std::fs::DirEntry);

impl DirEntry {
    /// Returns the full path to the entry.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.0.path()
    }

    /// Returns the bare file name of the entry.
    #[must_use]
    pub fn file_name(&self) -> OsString {
        self.0.file_name()
    }

    /// Queries metadata for the entry without following symlinks.
    pub fn metadata(&self) -> io::Result<Metadata> {
        self.0.metadata().map(Metadata::from_std)
    }

    /// Returns the file type of the entry.
    pub fn file_type(&self) -> io::Result<FileType> {
        self.0.file_type().map(FileType::from_std)
    }
}
