//! Aggregates bootstrap unit and integration-style test submodules.
//!
//! Focused modules cover environment formatting, migration workflows such as
//! `migration_disk_to_db` and `migration_rename`, and shared fixtures exposed
//! through `migration_support` so bootstrap coverage stays consolidated.

mod base_dir;
mod env_format;
mod migration;
mod migration_disk_to_db;
mod migration_rename;
mod migration_support;
mod pid_lock;
