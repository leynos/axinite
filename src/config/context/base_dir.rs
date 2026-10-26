//! Resolve the base directory from a captured environment snapshot.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::AXINITE_BASE_DIR_ENV;

pub(super) fn resolve_base_dir_snapshot(
    env_vars: &HashMap<String, String>,
    home_dir_fallback: Option<&Path>,
) -> PathBuf {
    if let Some(base_dir) = env_vars
        .get(AXINITE_BASE_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return base_dir;
    }

    env_vars
        .get("HOME")
        .or_else(|| env_vars.get("USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env_vars
                .get("HOMEDRIVE")
                .zip(env_vars.get("HOMEPATH"))
                .map(|(drive, path)| PathBuf::from(format!("{drive}{path}")))
        })
        .or_else(|| home_dir_fallback.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".axinite")
}
