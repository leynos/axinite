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

use crate::config::{Config, ConfigError, INJECTED_VARS, runtime_support};
use crate::settings::Settings;

use crate::llm::config::LlmConfig;

const AXINITE_BASE_DIR_ENV: &str = "AXINITE_BASE_DIR";

/// Snapshot of environment-backed configuration inputs.
#[derive(Clone, Debug)]
pub struct EnvContext {
    env_vars: HashMap<String, String>,
    secrets: HashMap<String, String>,
    base_dir: PathBuf,
}

/// Inputs used to re-resolve an LLM configuration after credential injection.
pub struct LlmResolutionContext<'a> {
    /// Environment snapshot containing the effective configuration values.
    pub env_context: &'a EnvContext,
    /// Optional settings store used to resolve provider settings.
    pub settings_store: Option<&'a (dyn crate::db::SettingsStore + Sync)>,
    /// User whose stored settings should be loaded.
    pub user_id: &'a str,
    /// Optional TOML overlay applied to stored settings.
    pub toml_path: Option<&'a std::path::Path>,
}

impl Default for EnvContext {
    fn default() -> Self {
        Self::for_testing(HashMap::new(), HashMap::new())
    }
}

impl EnvContext {
    /// Capture the current process environment and injected secret overlay.
    pub fn capture_ambient() -> Self {
        let (env_vars, secrets) = capture_ambient_inputs();
        let base_dir = resolve_base_dir_snapshot(&env_vars);
        Self {
            env_vars,
            secrets,
            base_dir,
        }
    }

    /// Construct an isolated context for tests or pure callers.
    pub fn for_testing(
        env_vars: HashMap<String, String>,
        secrets: HashMap<String, String>,
    ) -> Self {
        let base_dir = resolve_base_dir_snapshot(&env_vars);
        Self {
            env_vars,
            secrets,
            base_dir,
        }
    }

    /// Look up a raw environment value in the snapshot.
    ///
    /// Windows environment variable names are case-insensitive, so a
    /// mixed-case spelling such as `Path` still resolves there, matching the
    /// previous `std::env` behaviour. Unix names stay exact-case.
    fn lookup_env(&self, key: &str) -> Option<&str> {
        if let Some(value) = self.env_vars.get(key) {
            return Some(value.as_str());
        }
        #[cfg(windows)]
        {
            self.env_vars
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.as_str())
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Get a value from the snapshot, preferring env vars over injected secrets.
    ///
    /// Empty strings are treated as unset so they do not block fallback lookup.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lookup_env(key)
            .filter(|value| !value.is_empty())
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

    /// Whether either map has a non-empty value for `key`.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Whether the environment snapshot contains `key`, including empty values.
    ///
    /// Unlike [`Self::contains_key`], this ignores the secret overlay and does
    /// not treat an empty environment value as absent.
    pub(crate) fn contains_env_var(&self, key: &str) -> bool {
        self.lookup_env(key).is_some()
    }

    /// Raw environment value for `key`, including empty strings; ignores secrets.
    ///
    /// Windows lookups are case-insensitive, so a captured `Path` entry is
    /// found when callers ask for `PATH`.
    pub(crate) fn env_var_raw(&self, key: &str) -> Option<&str> {
        self.lookup_env(key)
    }

    /// Add or replace an env var in the snapshot.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let value = value.into();
        self.env_vars.insert(key.clone(), value);
        if key == AXINITE_BASE_DIR_ENV {
            self.base_dir = resolve_base_dir_snapshot(&self.env_vars);
        }
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

    /// Remove one injected secret without changing process-global state.
    pub fn remove_secret(&mut self, key: &str) {
        self.secrets.remove(key);
    }

    /// Merge multiple secret overlay values without mutating global state.
    pub fn merge_secrets(&mut self, secrets: HashMap<String, String>) {
        self.secrets.extend(secrets);
    }

    pub(crate) fn secret_values(&self) -> HashMap<String, String> {
        self.secrets.clone()
    }

    /// Resolve the effective Axinite base directory from the snapshot.
    pub fn axinite_base_dir(&self) -> PathBuf {
        self.base_dir.clone()
    }
}

impl Config {
    /// Re-resolve only the LLM config after credential injection.
    ///
    /// Called by `AppBuilder::init_secrets()` after injecting API keys into
    /// the env overlay. Only rebuilds `self.llm` — all other config fields
    /// are unaffected, preserving values from the initial config load (or
    /// from `Config::for_testing()` in test mode).
    pub async fn re_resolve_llm(
        &mut self,
        store: Option<&(dyn crate::db::SettingsStore + Sync)>,
        user_id: &str,
        toml_path: Option<&std::path::Path>,
    ) -> Result<(), ConfigError> {
        let env_context = EnvContext::capture_ambient();
        self.re_resolve_llm_with_context(LlmResolutionContext {
            env_context: &env_context,
            settings_store: store,
            user_id,
            toml_path,
        })
        .await
    }

    /// Re-resolve only the LLM config from an explicit environment snapshot.
    pub async fn re_resolve_llm_with_context(
        &mut self,
        context: LlmResolutionContext<'_>,
    ) -> Result<(), ConfigError> {
        let LlmResolutionContext {
            env_context,
            settings_store,
            user_id,
            toml_path,
        } = context;
        let settings = if let Some(store) = settings_store {
            let mut s = match store
                .get_all_settings(crate::db::UserId::from(user_id))
                .await
            {
                Ok(map) => Settings::from_db_map(&map),
                Err(_) => Settings::default(),
            };
            runtime_support::apply_toml_overlay(&mut s, toml_path)?;
            s
        } else {
            Settings::default()
        };
        self.llm = LlmConfig::resolve_from(env_context, &settings)?;
        Ok(())
    }
}

fn capture_ambient_inputs() -> (HashMap<String, String>, HashMap<String, String>) {
    #[expect(
        clippy::disallowed_methods,
        reason = "sole sanctioned ambient environment boundary; all other code reads via EnvContext"
    )]
    let env_vars = collect_utf8_env_vars(std::env::vars_os());
    let secrets = match INJECTED_VARS.lock() {
        Ok(map) => map.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    (env_vars, secrets)
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
        home.join(".axinite")
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join(".axinite")
    }
}

fn resolve_base_dir_snapshot(env_vars: &HashMap<String, String>) -> PathBuf {
    env_vars
        .get(AXINITE_BASE_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_base_dir)
}

#[cfg(test)]
mod tests {
    //! Unit tests for `EnvContext` environment and secret lookup precedence.

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
    fn contains_env_var_counts_empty_values_and_ignores_secrets() {
        let ctx = EnvContext::default()
            .with_env("EMPTY_BACKEND", "")
            .with_secret("SECRET_BACKEND", "postgres");

        assert!(ctx.contains_env_var("EMPTY_BACKEND"));
        assert!(!ctx.contains_env_var("SECRET_BACKEND"));
    }

    #[cfg(windows)]
    #[test]
    fn lookup_is_case_insensitive_on_windows() {
        let ctx = EnvContext::default().with_env("Path", r"C:\Windows\System32");

        assert_eq!(ctx.get("PATH"), Some(r"C:\Windows\System32"));
        assert!(ctx.contains_env_var("PATH"));
        assert_eq!(ctx.env_var_raw("PATH"), Some(r"C:\Windows\System32"));
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
        let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/tmp/axinite");

        assert_eq!(ctx.axinite_base_dir(), PathBuf::from("/tmp/axinite"));
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
