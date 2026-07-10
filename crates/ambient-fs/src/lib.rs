//! Deliberately-ambient filesystem access for the axinite host.
//!
//! The Whitaker `no_std_fs_operations` lint forbids direct `std::fs` use so
//! that filesystem access flows through capability-oriented handles
//! (`cap_std`) wherever a directory capability exists. The host application,
//! however, owns a set of genuinely ambient boundaries — user-scoped
//! configuration and data directories, PID lock files, workspace roots, and
//! extension staging areas — whose paths come from the environment rather
//! than from a capability.
//!
//! This crate confines that ambient access behind one auditable seam. It
//! mirrors the `std::fs` surface the host actually uses, delegating each
//! operation to `std::fs`, and is the only production crate excluded from
//! `no_std_fs_operations` in the root `dylint.toml`. New call sites should
//! prefer `cap_std` handles where a parent directory capability is
//! available, and reach for this crate only when the access is
//! ambient-by-design.

mod dir;
mod file;
mod fns;
mod meta;

pub use dir::{DirEntry, ReadDir};
pub use file::{File, OpenOptions};
pub use fns::{
    canonicalize, copy, create_dir, create_dir_all, metadata, read, read_dir, read_to_string,
    remove_dir, remove_dir_all, remove_file, rename, set_permissions, try_exists, write,
};
pub use meta::{FileType, Metadata, Permissions};
