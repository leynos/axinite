//! File handle and open-options wrappers around `std::fs`.
//!
//! The wrappers own a [`std::fs::File`] and re-implement the I/O traits by
//! delegation so that call sites never hold a bare `std::fs` handle.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::meta::{Metadata, Permissions};

/// A file handle, wrapping [`std::fs::File`].
#[derive(Debug)]
pub struct File(std::fs::File);

impl File {
    /// Opens an existing file in read-only mode.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        std::fs::File::open(path).map(Self)
    }

    /// Creates a new file, truncating it if it already exists.
    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        std::fs::File::create(path).map(Self)
    }

    /// Creates a new file, failing if it already exists.
    pub fn create_new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        std::fs::File::create_new(path).map(Self)
    }

    /// Returns a new [`OpenOptions`] builder.
    #[must_use]
    pub fn options() -> OpenOptions {
        OpenOptions::new()
    }

    /// Wraps an existing [`std::fs::File`].
    #[must_use]
    pub fn from_std(inner: std::fs::File) -> Self {
        Self(inner)
    }

    /// Unwraps into the underlying [`std::fs::File`].
    #[must_use]
    pub fn into_std(self) -> std::fs::File {
        self.0
    }

    /// Borrows the underlying [`std::fs::File`].
    #[must_use]
    pub fn as_std(&self) -> &std::fs::File {
        &self.0
    }

    /// Queries metadata about the underlying file.
    pub fn metadata(&self) -> io::Result<Metadata> {
        self.0.metadata().map(Metadata::from_std)
    }

    /// Truncates or extends the file to `size` bytes.
    pub fn set_len(&self, size: u64) -> io::Result<()> {
        self.0.set_len(size)
    }

    /// Changes the permissions on the underlying file.
    pub fn set_permissions(&self, perm: Permissions) -> io::Result<()> {
        self.0.set_permissions(perm.into_std())
    }

    /// Synchronises all in-memory data and metadata to disk.
    pub fn sync_all(&self) -> io::Result<()> {
        self.0.sync_all()
    }

    /// Synchronises in-memory data (not necessarily metadata) to disk.
    pub fn sync_data(&self) -> io::Result<()> {
        self.0.sync_data()
    }

    /// Creates an independently-owned handle to the same file.
    pub fn try_clone(&self) -> io::Result<Self> {
        self.0.try_clone().map(Self)
    }

    /// Takes an exclusive advisory lock, blocking until it is granted.
    pub fn lock_exclusive(&self) -> io::Result<()> {
        fs4::FileExt::lock_exclusive(&self.0)
    }

    /// Attempts an exclusive advisory lock without blocking.
    pub fn try_lock_exclusive(&self) -> io::Result<()> {
        fs4::FileExt::try_lock_exclusive(&self.0)
    }

    /// Takes a shared advisory lock, blocking until it is granted.
    pub fn lock_shared(&self) -> io::Result<()> {
        fs4::FileExt::lock_shared(&self.0)
    }

    /// Releases any advisory lock held on the file.
    pub fn unlock(&self) -> io::Result<()> {
        fs4::FileExt::unlock(&self.0)
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Read for &File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.0).read(buf)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Write for &File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.0).flush()
    }
}

impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl From<std::fs::File> for File {
    fn from(inner: std::fs::File) -> Self {
        Self(inner)
    }
}

/// Options controlling how a file is opened, wrapping
/// [`std::fs::OpenOptions`].
#[derive(Clone, Debug)]
pub struct OpenOptions(std::fs::OpenOptions);

impl OpenOptions {
    /// Creates a blank set of options.
    #[must_use]
    pub fn new() -> Self {
        Self(std::fs::OpenOptions::new())
    }

    /// Sets the option for read access.
    pub fn read(&mut self, read: bool) -> &mut Self {
        self.0.read(read);
        self
    }

    /// Sets the option for write access.
    pub fn write(&mut self, write: bool) -> &mut Self {
        self.0.write(write);
        self
    }

    /// Sets the option for append mode.
    pub fn append(&mut self, append: bool) -> &mut Self {
        self.0.append(append);
        self
    }

    /// Sets the option for truncating the file on open.
    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.0.truncate(truncate);
        self
    }

    /// Sets the option to create the file if it does not exist.
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.0.create(create);
        self
    }

    /// Sets the option to fail if the file already exists.
    pub fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.0.create_new(create_new);
        self
    }

    /// Sets the Unix mode bits applied when creating the file.
    #[cfg(unix)]
    pub fn mode(&mut self, mode: u32) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.0.mode(mode);
        self
    }

    /// Opens the file at `path` with these options.
    pub fn open<P: AsRef<Path>>(&self, path: P) -> io::Result<File> {
        self.0.open(path).map(File)
    }
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}
