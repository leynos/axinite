//! Explicit configuration input snapshot.
//!
//! `EnvContext` captures the process environment, parsed dotenv layers, and
//! injected secrets that participate in config resolution. Prefer
//! [`EnvContext::capture_ambient`] at startup, then pass the captured context
//! through `Config::from_context(...)` and other `*_from(...)` helpers when you
//! need deterministic precedence or isolated tests.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use self::base_dir::resolve_base_dir_snapshot;
use crate::config::INJECTED_VARS;
use crate::config::database::autodetect_libsql_backend;
use crate::config::dotenv_layers::{DOTENV_LAYERS, DotenvLayers};

mod ambient_presence;
mod base_dir;
mod restart;

const AXINITE_BASE_DIR_ENV: &str = "AXINITE_BASE_DIR";

#[derive(Debug)]
struct ProcessEnvSnapshot {
    env_vars: HashMap<String, String>,
    non_unicode_values: HashSet<String>,
    home_dir_fallback: Option<PathBuf>,
}

static PROCESS_ENV_SNAPSHOT: OnceLock<ProcessEnvSnapshot> = OnceLock::new();

/// Snapshot of environment-backed configuration inputs.
#[derive(Clone, Debug)]
pub struct EnvContext {
    env_vars: HashMap<String, String>,
    non_unicode_values: HashSet<String>,
    secrets: HashMap<String, String>,
    home_dir_fallback: Option<PathBuf>,
    base_dir: PathBuf,
    ambient_source: bool,
    explicit_env_overrides: HashMap<String, String>,
}

impl Default for EnvContext {
    fn default() -> Self {
        Self::for_testing(HashMap::new(), HashMap::new())
    }
}

impl EnvContext {
    /// Capture the process environment snapshot and injected secret overlay.
    ///
    /// When `DATABASE_BACKEND` is unset and a local libSQL database file
    /// exists, the snapshot also defaults the backend to `libsql`, so the
    /// process environment never needs to be mutated to carry that default.
    /// Process values are captured once; [`Self::refresh_dotenv_layers`] is
    /// the refresh point for file-backed values.
    pub fn capture_ambient() -> Self {
        let (process, secrets) = capture_ambient_inputs(true);
        let layers = dotenv_layers_snapshot();
        Self::from_dotenv_layers_with_non_unicode(
            process.env_vars.clone(),
            process.non_unicode_values.clone(),
            secrets,
            &layers,
            process.home_dir_fallback.as_deref(),
        )
    }

    /// Capture real process variables without dotenv layers or injected secrets.
    ///
    /// This is for process-level controls and test harness inputs that must not
    /// depend on an operator's `.env` files. Application configuration should
    /// use [`Self::capture_ambient`] instead.
    pub fn capture_process() -> Self {
        let (process, _) = capture_ambient_inputs(false);
        let env_vars = process.env_vars.clone();
        let non_unicode_values = process.non_unicode_values.clone();
        let home_dir_fallback = process.home_dir_fallback.clone();
        let base_dir = resolve_base_dir_snapshot(&env_vars, home_dir_fallback.as_deref());
        Self {
            env_vars,
            non_unicode_values,
            secrets: HashMap::new(),
            home_dir_fallback,
            base_dir,
            ambient_source: false,
            explicit_env_overrides: HashMap::new(),
        }
    }

    /// Re-read dotenv files and replace the process-wide parsed layer snapshot.
    ///
    /// A later ambient config load sees the replacement. Existing
    /// `EnvContext` values remain immutable snapshots.
    pub fn refresh_dotenv_layers() {
        let (process, _) = capture_ambient_inputs(false);
        let start_dir = std::env::current_dir().unwrap_or_else(|error| {
            tracing::warn!(%error, "Failed to read current directory for dotenv loading");
            PathBuf::from(".")
        });
        let cached_base_dir = crate::bootstrap::cached_axinite_base_dir();
        let layers = match cached_base_dir.as_deref() {
            Some(base_dir) => DotenvLayers::load_with_base_dir(
                &start_dir,
                &process.env_vars,
                Some(base_dir),
                process.home_dir_fallback.as_deref(),
            ),
            None => DotenvLayers::load(
                &start_dir,
                &process.env_vars,
                process.home_dir_fallback.as_deref(),
            ),
        };
        tracing::debug!(
            bootstrap_path = ?layers.bootstrap_path(),
            "Refreshed parsed dotenv layers"
        );
        let mut snapshot = DOTENV_LAYERS
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *snapshot = Arc::new(layers);
    }

    /// Refresh ambient dotenv inputs for a reload while preserving explicit
    /// values and secrets attached to this context.
    pub(crate) fn refreshed_ambient_snapshot(&self) -> Self {
        if !self.ambient_source {
            return self.clone();
        }

        Self::refresh_dotenv_layers();
        let (process, mut secrets) = capture_ambient_inputs(true);
        secrets.extend(self.secrets.clone());
        let layers = dotenv_layers_snapshot();
        let mut refreshed = Self::from_dotenv_layers_with_non_unicode(
            process.env_vars.clone(),
            process.non_unicode_values.clone(),
            secrets,
            &layers,
            process.home_dir_fallback.as_deref(),
        );
        for (key, value) in &self.explicit_env_overrides {
            refreshed.env_vars.insert(key.clone(), value.clone());
            refreshed.non_unicode_values.remove(key);
            if key == AXINITE_BASE_DIR_ENV {
                refreshed.base_dir = resolve_base_dir_snapshot(
                    &refreshed.env_vars,
                    refreshed.home_dir_fallback.as_deref(),
                );
            }
        }
        refreshed
    }

    /// Build an ambient-style context from explicit process inputs and dotenv
    /// layers. The process map wins over the current-directory and bootstrap
    /// maps; injected secrets remain the final fallback in [`Self::get`].
    #[cfg(test)]
    pub(crate) fn from_dotenv_layers(
        process: HashMap<String, String>,
        secrets: HashMap<String, String>,
        layers: &DotenvLayers,
    ) -> Self {
        Self::from_dotenv_layers_with_non_unicode(process, HashSet::new(), secrets, layers, None)
    }

    fn from_dotenv_layers_with_non_unicode(
        process: HashMap<String, String>,
        non_unicode_values: HashSet<String>,
        secrets: HashMap<String, String>,
        layers: &DotenvLayers,
        home_dir_fallback: Option<&std::path::Path>,
    ) -> Self {
        let mut env_vars = layers.bootstrap_values().clone();
        env_vars.extend(layers.cwd_values().clone());
        env_vars.extend(process);

        let base_dir = layers
            .base_dir()
            .map(PathBuf::from)
            .unwrap_or_else(|| resolve_base_dir_snapshot(&env_vars, home_dir_fallback));
        autodetect_libsql_backend(&mut env_vars, &base_dir);

        Self {
            env_vars,
            non_unicode_values,
            secrets,
            home_dir_fallback: home_dir_fallback.map(PathBuf::from),
            base_dir,
            ambient_source: true,
            explicit_env_overrides: HashMap::new(),
        }
    }

    /// Construct an isolated context for tests or pure callers.
    pub fn for_testing(
        env_vars: HashMap<String, String>,
        secrets: HashMap<String, String>,
    ) -> Self {
        let base_dir = resolve_base_dir_snapshot(&env_vars, None);
        Self {
            env_vars,
            non_unicode_values: HashSet::new(),
            secrets,
            home_dir_fallback: None,
            base_dir,
            ambient_source: false,
            explicit_env_overrides: HashMap::new(),
        }
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

    /// Resolve one current ambient value without constructing a full config
    /// snapshot or running derived defaults such as libSQL autodetection.
    ///
    /// The process environment is captured once under the no-runtime-mutation
    /// policy. Parsed dotenv layers and injected secrets stay refreshable, so
    /// callers such as the sandbox credential resolver see their latest values
    /// without cloning every environment map on each request.
    pub(crate) fn get_ambient_owned(key: &str) -> Option<String> {
        let (process, _) = capture_ambient_inputs(false);
        let layers = dotenv_layers_snapshot();
        let secrets = INJECTED_VARS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ambient_value_from_layers(key, &process.env_vars, &layers, &secrets)
    }

    /// Whether any environment or secret layer contains the key, including
    /// empty and non-Unicode process values.
    pub fn contains_key(&self, key: &str) -> bool {
        self.env_vars.contains_key(key)
            || self.non_unicode_values.contains(key)
            || self.secrets.contains_key(key)
    }

    /// Whether a non-empty process or dotenv value exists without consulting
    /// the injected-secret fallback.
    pub(crate) fn has_non_empty_env_value(&self, key: &str) -> bool {
        self.env_vars
            .get(key)
            .is_some_and(|value| !value.is_empty())
    }

    /// Return the raw environment value, including an empty value.
    #[cfg(feature = "postgres")]
    pub(crate) fn raw_env_value(&self, key: &str) -> Option<&str> {
        self.env_vars.get(key).map(String::as_str)
    }

    /// Whether a process environment value was present but not valid UTF-8.
    #[cfg(feature = "postgres")]
    pub(crate) fn has_non_unicode_value(&self, key: &str) -> bool {
        self.non_unicode_values.contains(key)
    }

    /// Add or replace an env var in the snapshot.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let value = value.into();
        self.env_vars.insert(key.clone(), value.clone());
        self.non_unicode_values.remove(&key);
        if self.ambient_source {
            self.explicit_env_overrides.insert(key.clone(), value);
        }
        if key == AXINITE_BASE_DIR_ENV {
            self.base_dir =
                resolve_base_dir_snapshot(&self.env_vars, self.home_dir_fallback.as_deref());
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
fn capture_ambient_inputs(
    include_secrets: bool,
) -> (&'static ProcessEnvSnapshot, HashMap<String, String>) {
    let process = PROCESS_ENV_SNAPSHOT.get_or_init(|| {
        #[expect(
            clippy::disallowed_methods,
            reason = "sole sanctioned ambient environment boundary; all other code reads via EnvContext"
        )]
        let (env_vars, non_unicode_values) = collect_utf8_env_vars(std::env::vars_os());
        ProcessEnvSnapshot {
            env_vars,
            non_unicode_values,
            home_dir_fallback: dirs::home_dir(),
        }
    });
    let secrets = if include_secrets {
        INJECTED_VARS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    } else {
        HashMap::new()
    };
    (process, secrets)
}

fn ambient_value_from_layers(
    key: &str,
    process: &HashMap<String, String>,
    layers: &DotenvLayers,
    secrets: &HashMap<String, String>,
) -> Option<String> {
    process
        .get(key)
        .or_else(|| layers.cwd_values().get(key))
        .or_else(|| layers.bootstrap_values().get(key))
        .filter(|value| !value.is_empty())
        .or_else(|| secrets.get(key).filter(|value| !value.is_empty()))
        .cloned()
}

fn dotenv_layers_snapshot() -> Arc<DotenvLayers> {
    DOTENV_LAYERS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn collect_utf8_env_vars(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> (HashMap<String, String>, HashSet<String>) {
    let mut env_vars = HashMap::new();
    let mut non_unicode_values = HashSet::new();
    for (key, value) in vars {
        let Ok(key) = key.into_string() else {
            continue;
        };
        match value.into_string() {
            Ok(value) => {
                env_vars.insert(key, value);
            }
            Err(_) => {
                non_unicode_values.insert(key);
            }
        }
    }
    (env_vars, non_unicode_values)
}

#[cfg(test)]
#[path = "context/tests.rs"]
mod tests;
