//! Registers test sub-modules for the bootstrap subsystem and exposes
//! shared migration test fixtures via the `migration_support` module.

mod base_dir;
mod env_format;
mod migration;
mod migration_disk_to_db;
mod migration_rename;
mod migration_support;
mod pid_lock;
