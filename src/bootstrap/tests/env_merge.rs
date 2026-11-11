//! Property-based tests for merging a saved `.env` into an `EnvContext`.

use std::collections::HashMap;

use proptest::prelude::*;

use super::super::*;
use crate::config::EnvContext;

fn env_key() -> impl Strategy<Value = String> {
    "[A-Z][A-Z0-9_]{0,12}"
}

/// Values include the empty string: an empty snapshot entry still counts as a
/// present environment variable and must block the file value.
fn env_value() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_.-]{0,16}"
}

fn env_pairs() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::collection::vec((env_key(), env_value()), 0..8)
}

/// Reference merge for the raw environment map: existing values are preserved
/// and the first new assignment from the file wins, matching dotenv
/// precedence. This mirrors `merge_axinite_env_into_context` without sharing
/// its implementation.
fn reference_merge(
    snapshot_values: &HashMap<String, String>,
    file_pairs: &[(String, String)],
) -> HashMap<String, String> {
    let mut merged = snapshot_values.clone();
    for (key, value) in file_pairs {
        merged.entry(key.clone()).or_insert_with(|| value.clone());
    }
    merged
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The snapshot wins over the file, and where the snapshot has no value
    /// the first declaration in the file wins, matching dotenv precedence.
    /// Raw environment values and the secret overlay are both compared with a
    /// reference merge, so an empty override, a repeated assignment, or a
    /// secret clobbered by the file is detected.
    #[test]
    fn prop_merge_prefers_snapshot_then_first_file_value(
        saved in env_pairs(),
        snapshot in env_pairs(),
        secrets in env_pairs(),
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
        let secret_values: HashMap<String, String> = secrets.into_iter().collect();
        let mut ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", base_dir.clone());
        for (key, value) in &snapshot_values {
            ctx = ctx.with_env(key.clone(), value.clone());
        }
        for (key, value) in &secret_values {
            ctx = ctx.with_secret(key.clone(), value.clone());
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

        let mut expected_env = snapshot_values.clone();
        expected_env.insert("AXINITE_BASE_DIR".to_string(), base_dir.clone());
        let expected_env = reference_merge(&expected_env, &file_pairs);

        for (key, value) in &expected_env {
            prop_assert_eq!(
                merged.env_var_raw(key),
                Some(value.as_str()),
                "raw environment value for {} must match the reference merge",
                key
            );
        }
        prop_assert_eq!(merged.secret_values(), secret_values);
    }
}
