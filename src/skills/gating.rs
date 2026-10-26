//! Requirements gating for skills.
//!
//! Checks that a skill's declared requirements (binaries, environment variables,
//! config files) are satisfied before the skill is loaded.

use crate::config::EnvContext;
use crate::skills::GatingRequirements;

/// Result of a gating check.
#[derive(Debug)]
pub struct GatingResult {
    /// Whether all requirements passed.
    pub passed: bool,
    /// Descriptions of failed requirements.
    pub failures: Vec<String>,
}

/// Async wrapper around [`check_requirements_sync`] that offloads blocking
/// subprocess calls (`which`/`where`) to a blocking thread pool via
/// `tokio::task::spawn_blocking`.
pub async fn check_requirements(requirements: &GatingRequirements) -> GatingResult {
    let requirements = requirements.clone();
    tokio::task::spawn_blocking(move || check_requirements_sync(&requirements))
        .await
        .unwrap_or_else(|e| {
            let message = if e.is_panic() {
                format!("gating check panicked: {}", e)
            } else if e.is_cancelled() {
                format!("gating check task was cancelled: {}", e)
            } else {
                format!("gating check failed to join: {}", e)
            };
            tracing::error!("{}", message);
            GatingResult {
                passed: false,
                failures: vec![message],
            }
        })
}

/// Check whether gating requirements are satisfied (synchronous).
///
/// - `bins`: checks that each binary is findable via `which` (PATH lookup).
/// - `env`: checks that each environment variable is set.
/// - `config`: checks that each config file path exists.
///
/// Skills that fail gating should be logged and skipped, not loaded.
///
/// This is the synchronous implementation; prefer the async [`check_requirements`]
/// wrapper when calling from async contexts to avoid blocking the tokio runtime.
pub fn check_requirements_sync(requirements: &GatingRequirements) -> GatingResult {
    check_requirements_from_context(requirements, &EnvContext::capture_ambient())
}

/// Check requirements against an explicit snapshot. Environment presence
/// satisfies gating even for an empty value; this does not guarantee that a
/// spawned child process inherits values held only in dotenv layers.
fn check_requirements_from_context(
    requirements: &GatingRequirements,
    env_context: &EnvContext,
) -> GatingResult {
    let mut failures = Vec::new();

    for bin in &requirements.bins {
        if !binary_exists(bin) {
            failures.push(format!("required binary not found: {}", bin));
        }
    }

    for var in &requirements.env {
        if !env_context.contains_key(var) {
            failures.push(format!("required env var not set: {}", var));
        }
    }

    for path in &requirements.config {
        if !std::path::Path::new(path).exists() {
            failures.push(format!("required config not found: {}", path));
        }
    }

    GatingResult {
        passed: failures.is_empty(),
        failures,
    }
}

/// Check if a binary exists on PATH using `std::process::Command`.
pub(crate) fn binary_exists(name: &str) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("which")
            .arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(windows)]
    {
        std::process::Command::new("where")
            .arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for skill gating requirement checks.

    use super::*;

    #[test]
    fn test_empty_requirements_pass() {
        let req = GatingRequirements::default();
        let result = check_requirements_sync(&req);
        assert!(result.passed);
        assert!(result.failures.is_empty());
    }

    #[test]
    fn test_missing_binary_fails() {
        let req = GatingRequirements {
            bins: vec!["__axinite_nonexistent_binary_xyz__".to_string()],
            ..Default::default()
        };
        let result = check_requirements_sync(&req);
        assert!(!result.passed);
        assert_eq!(result.failures.len(), 1);
        assert!(result.failures[0].contains("binary not found"));
    }

    #[test]
    fn test_missing_env_var_fails() {
        let req = GatingRequirements {
            env: vec!["__AXINITE_TEST_NONEXISTENT_VAR__".to_string()],
            ..Default::default()
        };
        let result = check_requirements_sync(&req);
        assert!(!result.passed);
        assert!(result.failures[0].contains("env var not set"));
    }

    #[test]
    fn test_present_env_var_passes() {
        let req = GatingRequirements {
            env: vec!["AXINITE_TEST_GATE_KEY".to_string()],
            ..Default::default()
        };
        let context = EnvContext::default().with_env("AXINITE_TEST_GATE_KEY", "value");
        let result = check_requirements_from_context(&req, &context);
        assert!(result.passed);
    }

    #[test]
    fn present_empty_env_var_passes_gating() {
        let req = GatingRequirements {
            env: vec!["AXINITE_TEST_GATE_EMPTY".to_string()],
            ..Default::default()
        };
        let context = EnvContext::default().with_env("AXINITE_TEST_GATE_EMPTY", "");

        let result = check_requirements_from_context(&req, &context);

        assert!(result.passed);
        assert!(result.failures.is_empty());
    }

    #[test]
    fn test_missing_config_fails() {
        let req = GatingRequirements {
            config: vec!["/nonexistent/path/axinite_test.conf".to_string()],
            ..Default::default()
        };
        let result = check_requirements_sync(&req);
        assert!(!result.passed);
        assert!(result.failures[0].contains("config not found"));
    }

    #[test]
    fn test_multiple_mixed_requirements() {
        let req = GatingRequirements {
            bins: vec!["__no_such_bin__".to_string()],
            env: vec!["__NO_SUCH_VAR__".to_string()],
            config: vec!["/no/such/file".to_string()],
        };
        let result = check_requirements_sync(&req);
        assert!(!result.passed);
        assert_eq!(result.failures.len(), 3);
    }
}
