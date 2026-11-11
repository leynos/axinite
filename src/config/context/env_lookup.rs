//! Case-aware lookup helpers for captured environment names.

use std::collections::{HashMap, HashSet};

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
