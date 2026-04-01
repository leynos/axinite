//! Explicit configuration input snapshot.
//!
//! `EnvContext` captures the environment inputs that participate in config
//! resolution so callers can build configuration from explicit data instead of
//! relying on ambient process state. Prefer [`EnvContext::capture_ambient`] at
//! startup, then pass the captured context through `Config::from_context(...)`
//! and other `*_from(...)` helpers when you need deterministic precedence or
//! isolated tests.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::config::INJECTED_VARS;

const IRONCLAW_BASE_DIR_ENV: &str = "IRONCLAW_BASE_DIR";

/// Snapshot of environment-backed configuration inputs.
#[derive(Clone, Debug, Default)]
pub struct EnvContext {
    env_vars: HashMap<String, String>,
    secrets: HashMap<String, String>,
}

impl EnvContext {
    /// Capture the current process environment and injected secret overlay.
    pub fn capture_ambient() -> Self {
        let env_vars = collect_utf8_env_vars(std::env::vars_os());
        let secrets = match INJECTED_VARS.lock() {
            Ok(map) => map.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        Self { env_vars, secrets }
    }

    /// Construct an isolated context for tests or pure callers.
    pub fn for_testing(
        env_vars: HashMap<String, String>,
        secrets: HashMap<String, String>,
    ) -> Self {
        Self { env_vars, secrets }
    }

    /// Get a value from the snapshot, preferring env vars over injected secrets.
    ///
    /// Empty strings are treated as unset so they do not block fallback lookup.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.env_vars
            .get(key)
            .filter(|value| !value.is_empty())
            .map(String::as_str)
            .or_else(|| {
                self.secrets
                    .get(key)
                    .filter(|value| !value.is_empty())
                    .map(String::as_str)
            })
    }

    /// Owned convenience wrapper around [`Self::get`].
    pub fn get_owned(&self, key: &str) -> Option<String> {
        self.get(key).map(str::to_string)
    }

    /// Whether either map contains the key.
    pub fn contains_key(&self, key: &str) -> bool {
        self.env_vars.contains_key(key) || self.secrets.contains_key(key)
    }

    /// Add or replace an env var in the snapshot.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Add or replace an injected secret in the snapshot.
    pub fn with_secret(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.secrets.insert(key.into(), value.into());
        self
    }

    /// Insert one secret overlay value without mutating global state.
    pub fn inject_secret(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.secrets.insert(key.into(), value.into());
    }

    /// Merge multiple secret overlay values without mutating global state.
    pub fn merge_secrets(&mut self, secrets: HashMap<String, String>) {
        self.secrets.extend(secrets);
    }

    /// Resolve the effective IronClaw base directory from the snapshot.
    pub fn ironclaw_base_dir(&self) -> PathBuf {
        self.env_vars
            .get(IRONCLAW_BASE_DIR_ENV)
            .map(PathBuf::from)
            .map(|path| {
                if path.as_os_str().is_empty() {
                    default_base_dir()
                } else {
                    path
                }
            })
            .unwrap_or_else(default_base_dir)
    }
}

fn collect_utf8_env_vars(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> HashMap<String, String> {
    vars.into_iter()
        .filter_map(
            |(key, value)| match (key.into_string(), value.into_string()) {
                (Ok(key), Ok(value)) => Some((key, value)),
                _ => None,
            },
        )
        .collect()
}

fn default_base_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        home.join(".ironclaw")
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join(".ironclaw")
    }
}

#[cfg(test)]
mod tests {
    use super::EnvContext;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn get_prefers_env_over_secret() {
        let ctx = EnvContext::for_testing(
            HashMap::from([(String::from("OPENAI_API_KEY"), String::from("env"))]),
            HashMap::from([(String::from("OPENAI_API_KEY"), String::from("secret"))]),
        );

        assert_eq!(ctx.get("OPENAI_API_KEY"), Some("env"));
    }

    #[test]
    fn empty_env_value_falls_back_to_secret() {
        let ctx = EnvContext::for_testing(
            HashMap::from([(String::from("OPENAI_API_KEY"), String::new())]),
            HashMap::from([(String::from("OPENAI_API_KEY"), String::from("secret"))]),
        );

        assert_eq!(ctx.get("OPENAI_API_KEY"), Some("secret"));
    }

    #[test]
    fn builder_helpers_populate_context() {
        let ctx = EnvContext::default()
            .with_env("A", "1")
            .with_secret("B", "2");

        assert_eq!(ctx.get("A"), Some("1"));
        assert_eq!(ctx.get("B"), Some("2"));
    }

    #[test]
    fn base_dir_uses_snapshot_override() {
        let ctx = EnvContext::default().with_env("IRONCLAW_BASE_DIR", "/tmp/axinite");

        assert_eq!(ctx.ironclaw_base_dir(), PathBuf::from("/tmp/axinite"));
    }

    #[cfg(unix)]
    #[test]
    fn ambient_snapshot_skips_non_utf8_entries() {
        use super::collect_utf8_env_vars;
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let env_vars = collect_utf8_env_vars([
            (OsString::from("VALID_KEY"), OsString::from("valid")),
            (
                OsString::from_vec(b"BAD\xffKEY".to_vec()),
                OsString::from("ignored"),
            ),
            (
                OsString::from("BAD_VALUE"),
                OsString::from_vec(b"bad\xffvalue".to_vec()),
            ),
        ]);

        assert_eq!(env_vars.get("VALID_KEY"), Some(&String::from("valid")));
        assert!(!env_vars.contains_key("BAD_VALUE"));
        assert_eq!(env_vars.len(), 1);
    }
}
