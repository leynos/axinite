//! Case-aware lookup helpers for captured environment names.

use std::collections::{HashMap, HashSet};

pub(super) fn normalize_env_key(key: &str) -> String {
    #[cfg(windows)]
    {
        key.to_ascii_uppercase()
    }
    #[cfg(not(windows))]
    {
        key.to_string()
    }
}

pub(super) fn merge_env_layer(
    target: &mut HashMap<String, String>,
    layer: &HashMap<String, String>,
) {
    for (key, value) in layer {
        target.insert(normalize_env_key(key), value.clone());
    }
}

pub(super) fn insert_env_value(target: &mut HashMap<String, String>, key: &str, value: String) {
    target.insert(normalize_env_key(key), value);
}

pub(super) fn lookup_map_value<'a>(
    map: &'a HashMap<String, String>,
    key: &str,
) -> Option<&'a String> {
    let value = map.get(key);
    #[cfg(windows)]
    {
        value.or_else(|| {
            map.iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value)
        })
    }
    #[cfg(not(windows))]
    {
        value
    }
}

pub(super) fn contains_env_key(keys: &HashSet<String>, key: &str) -> bool {
    keys.contains(key) || {
        #[cfg(windows)]
        {
            keys.iter().any(|name| name.eq_ignore_ascii_case(key))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}
