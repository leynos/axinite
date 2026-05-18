//! Registers bootstrap test sub-modules and groups related coverage.
//! `base_dir` covers base directory resolution, `env_format` covers
//! environment file formatting, and `pid_lock` covers PID locking.
//! `migration`, `migration_disk_to_db`, and `migration_rename` cover bootstrap
//! migration workflows, with shared migration fixtures exposed via
//! `migration_support`.

mod base_dir;
mod env_format;
mod migration;
mod migration_disk_to_db;
mod migration_rename;
mod migration_support;
mod pid_lock;
