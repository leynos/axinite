//! Resolve the base directory from a captured environment snapshot.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::AXINITE_BASE_DIR_ENV;
use super::env_lookup::lookup_map_value;

pub(super) fn resolve_base_dir_snapshot(
    env_vars: &HashMap<String, String>,
    home_dir_fallback: Option<&Path>,
) -> PathBuf {
    if let Some(base_dir) = lookup_map_value(env_vars, AXINITE_BASE_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return base_dir;
    }

    lookup_map_value(env_vars, "HOME")
        .or_else(|| lookup_map_value(env_vars, "USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            lookup_map_value(env_vars, "HOMEDRIVE")
                .zip(lookup_map_value(env_vars, "HOMEPATH"))
                .map(|(drive, path)| PathBuf::from(format!("{drive}{path}")))
        })
        .or_else(|| home_dir_fallback.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".axinite")
}
