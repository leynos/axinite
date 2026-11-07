//! Unit tests for the shell command execution tool.

mod approval;
mod env_scrubbing;
mod execution;
mod policy;

use tempfile::TempDir;

use crate::config::EnvContext;

/// Build a minimal environment snapshot for shell execution tests.
fn shell_context() -> anyhow::Result<(TempDir, EnvContext)> {
    let home_dir = tempfile::tempdir()?;
    let context = EnvContext::default()
        .with_env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .with_env("HOME", home_dir.path().to_string_lossy());
    Ok((home_dir, context))
}
