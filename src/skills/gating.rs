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
    let failures = missing_binary_failures(&requirements.bins)
        .chain(missing_env_failures(&requirements.env, env_context))
        .chain(missing_config_failures(&requirements.config))
        .collect::<Vec<_>>();

    GatingResult {
        passed: failures.is_empty(),
        failures,
    }
}

fn missing_binary_failures(bins: &[String]) -> impl Iterator<Item = String> + '_ {
    bins.iter()
        .filter(|bin| !binary_exists(bin))
        .map(|bin| format!("required binary not found: {bin}"))
}

fn missing_env_failures<'a>(
    variables: &'a [String],
    env_context: &'a EnvContext,
) -> impl Iterator<Item = String> + 'a {
    variables
        .iter()
        .filter(|variable| !env_context.contains_key(variable))
        .map(|variable| format!("required env var not set: {variable}"))
}

fn missing_config_failures(paths: &[String]) -> impl Iterator<Item = String> + '_ {
    paths
        .iter()
        .filter(|path| !std::path::Path::new(path).exists())
        .map(|path| format!("required config not found: {path}"))
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
    fn empty_requirements_pass() {
        let req = GatingRequirements::default();
        let result = check_requirements_from_context(&req, &EnvContext::default());
        assert!(result.passed);
        assert!(result.failures.is_empty());
    }

    #[rstest::rstest]
    #[case(None, false)]
    #[case(Some(""), true)]
    #[case(Some("value"), true)]
    fn env_requirement_uses_presence(#[case] value: Option<&str>, #[case] expected_to_pass: bool) {
        let context = value.map_or_else(EnvContext::default, |value| {
            EnvContext::default().with_env("AXINITE_TEST_GATE_KEY", value)
        });
        let req = GatingRequirements {
            env: vec!["AXINITE_TEST_GATE_KEY".to_string()],
            ..Default::default()
        };
        let result = check_requirements_from_context(&req, &context);

        assert_eq!(result.passed, expected_to_pass);
        assert_eq!(result.failures.is_empty(), expected_to_pass);
    }

    #[test]
    fn missing_requirements_report_each_kind() {
        let req = GatingRequirements {
            bins: vec!["__no_such_bin__".to_string()],
            env: vec!["AXINITE_TEST_MISSING_GATE_KEY".to_string()],
            config: vec!["/no/such/file".to_string()],
        };
        let result = check_requirements_from_context(&req, &EnvContext::default());

        assert!(!result.passed);
        assert_eq!(result.failures.len(), 3);
        assert!(
            result
                .failures
                .iter()
                .any(|failure| failure.contains("binary not found"))
        );
        assert!(
            result
                .failures
                .iter()
                .any(|failure| failure.contains("env var not set"))
        );
        assert!(
            result
                .failures
                .iter()
                .any(|failure| failure.contains("config not found"))
        );
    }
}
