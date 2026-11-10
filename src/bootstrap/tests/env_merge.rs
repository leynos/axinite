//! Property-based tests for merging a saved `.env` into an `EnvContext`.

use std::collections::HashMap;

use proptest::prelude::*;

use super::super::*;
use crate::config::EnvContext;

fn env_key() -> impl Strategy<Value = String> {
    "[A-Z][A-Z0-9_]{0,12}"
}

fn env_value() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_.-]{1,16}"
}

fn env_pairs() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::collection::vec((env_key(), env_value()), 0..8)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The snapshot wins over the file, and where the snapshot has no value
    /// the first declaration in the file wins, matching dotenv precedence.
    #[test]
    fn prop_merge_prefers_snapshot_then_first_file_value(
        saved in env_pairs(),
        snapshot in env_pairs(),
    ) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let base_dir = dir.path().display().to_string();

        // Keys are generated uppercase, so lookups are identical on every
        // platform. `AXINITE_BASE_DIR` is reserved for pointing the merge at
        // this case's temporary directory.
        let snapshot_values: HashMap<String, String> = snapshot
            .into_iter()
            .filter(|(key, _)| key != "AXINITE_BASE_DIR")
            .collect();
        let mut ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", base_dir.clone());
        for (key, value) in &snapshot_values {
            ctx = ctx.with_env(key.clone(), value.clone());
        }

        let file_pairs: Vec<(String, String)> = saved
            .into_iter()
            .filter(|(key, _)| key != "AXINITE_BASE_DIR")
            .collect();
        let mut contents = String::new();
        for (key, value) in &file_pairs {
            contents.push_str(&format!("{key}={value}\n"));
        }
        ambient_fs::write(dir.path().join(".env"), contents).expect("write .env");

        let merged = merge_axinite_env_into_context(&ctx).expect("merge should succeed");

        prop_assert_eq!(merged.get("AXINITE_BASE_DIR"), Some(base_dir.as_str()));
        for (key, value) in &snapshot_values {
            prop_assert_eq!(merged.get(key), Some(value.as_str()));
        }
        for (index, (key, value)) in file_pairs.iter().enumerate() {
            if snapshot_values.contains_key(key) {
                continue;
            }
            let first_declaration =
                !file_pairs[..index].iter().any(|(earlier, _)| earlier == key);
            if first_declaration {
                prop_assert_eq!(merged.get(key), Some(value.as_str()));
            }
        }
    }
}
