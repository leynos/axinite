//! Resolve ambient key presence without cloning the entire configuration snapshot.

use std::collections::HashMap;
use std::path::Path;

use super::{
    EnvContext, ProcessEnvSnapshot, capture_ambient_inputs, dotenv_layers_snapshot,
    resolve_base_dir_snapshot,
};
use crate::config::INJECTED_VARS;
use crate::config::database::default_libsql_path_in;
use crate::config::dotenv_layers::DotenvLayers;

impl EnvContext {
    /// Check presence in the current ambient layers, including empty values.
    ///
    /// This matches `capture_ambient().contains_key(key)` without cloning the
    /// maps. The derived database default is checked only for its own key.
    pub(crate) fn ambient_contains_key(key: &str) -> bool {
        let (process, _) = capture_ambient_inputs(false);
        let layers = dotenv_layers_snapshot();
        let secrets = INJECTED_VARS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        contains_in_layers(key, process, &layers, &secrets)
    }
}

pub(super) fn contains_in_layers(
    key: &str,
    process: &ProcessEnvSnapshot,
    layers: &DotenvLayers,
    secrets: &HashMap<String, String>,
) -> bool {
    let present_in_layers = [
        &process.env_vars,
        layers.cwd_values(),
        layers.bootstrap_values(),
        secrets,
    ]
    .into_iter()
    .any(|layer| layer.contains_key(key));
    if present_in_layers || process.non_unicode_values.contains(key) {
        return true;
    }
    if key != "DATABASE_BACKEND" {
        return false;
    }
    let base_dir = layers.base_dir().map(Path::to_path_buf).unwrap_or_else(|| {
        let mut env_vars = layers.bootstrap_values().clone();
        env_vars.extend(layers.cwd_values().clone());
        env_vars.extend(process.env_vars.clone());
        resolve_base_dir_snapshot(&env_vars, process.home_dir_fallback.as_deref())
    });
    default_libsql_path_in(&base_dir).exists()
}
