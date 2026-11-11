//! Capture the process environment once and read the replaceable dotenv layer.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use super::env_lookup::{lookup_map_value, normalize_env_key};
use crate::config::INJECTED_VARS;
use crate::config::dotenv_layers::{DOTENV_LAYERS, DotenvLayers};

#[derive(Debug)]
pub(super) struct ProcessEnvSnapshot {
    pub(super) env_vars: HashMap<String, String>,
    pub(super) non_unicode_values: HashSet<String>,
    pub(super) home_dir_fallback: Option<PathBuf>,
}

static PROCESS_ENV_SNAPSHOT: OnceLock<ProcessEnvSnapshot> = OnceLock::new();

pub(super) fn capture_ambient_inputs(
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

pub(super) fn ambient_value_from_layers(
    key: &str,
    process: &HashMap<String, String>,
    layers: &DotenvLayers,
    secrets: &HashMap<String, String>,
) -> Option<String> {
    lookup_map_value(process, key)
        .or_else(|| lookup_map_value(layers.cwd_values(), key))
        .or_else(|| lookup_map_value(layers.bootstrap_values(), key))
        .filter(|value| !value.is_empty())
        .or_else(|| lookup_map_value(secrets, key).filter(|value| !value.is_empty()))
        .cloned()
}

pub(super) fn merge_refreshed_secrets(
    mut current: HashMap<String, String>,
    explicit: &HashMap<String, String>,
) -> HashMap<String, String> {
    current.extend(explicit.clone());
    current
}

pub(super) fn dotenv_layers_snapshot() -> Arc<DotenvLayers> {
    DOTENV_LAYERS.snapshot()
}

pub(super) fn collect_utf8_env_vars(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> (HashMap<String, String>, HashSet<String>) {
    let mut env_vars = HashMap::new();
    let mut non_unicode_values = HashSet::new();
    for (key, value) in vars {
        let Ok(key) = key.into_string() else {
            continue;
        };
        let key = normalize_env_key(&key);
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
