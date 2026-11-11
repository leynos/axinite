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

/// One merge scenario with deliberate key overlap between the snapshot and the
/// file, plus repeated file assignments and file-only keys.
#[derive(Debug)]
struct MergeCase {
    snapshot: Vec<(String, String)>,
    file: Vec<(String, String)>,
    secrets: Vec<(String, String)>,
}

/// Generate a scenario whose file always re-assigns every snapshot key, so the
/// snapshot-wins rule is exercised rather than left to accidental collisions.
/// The first shared key is also assigned again later in the file, so the
/// first-declaration-wins rule is exercised too.
fn merge_case() -> impl Strategy<Value = MergeCase> {
    prop::collection::vec(env_key(), 0..6).prop_flat_map(|keys| {
        let count = keys.len();
        (
            prop::collection::vec(env_value(), count..=count),
            prop::collection::vec(env_value(), count..=count),
            prop::collection::vec(env_value(), 0..3),
            prop::collection::vec((env_key(), env_value()), 0..3),
            prop::collection::vec((env_key(), env_value()), 0..3),
        )
            .prop_map(
                move |(snapshot_values, file_values, repeat_values, file_only, secrets)| {
                    let snapshot: Vec<(String, String)> =
                        keys.iter().cloned().zip(snapshot_values).collect();
                    let mut file: Vec<(String, String)> =
                        keys.iter().cloned().zip(file_values).collect();
                    if let Some(first) = keys.first() {
                        file.extend(
                            repeat_values
                                .into_iter()
                                .map(|value| (first.clone(), value)),
                        );
                    }
                    file.extend(file_only);
                    MergeCase {
                        snapshot,
                        file,
                        secrets,
                    }
                },
            )
    })
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
    /// Every case re-assigns the snapshot keys in the file and repeats one
    /// file key, so both rules are exercised deliberately. Raw environment
    /// values and the secret overlay are both compared with a reference merge,
    /// so an empty override, a repeated assignment, or a secret clobbered by
    /// the file is detected.
    #[test]
    fn prop_merge_prefers_snapshot_then_first_file_value(case in merge_case()) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let base_dir = dir.path().display().to_string();

        // Keys are generated uppercase, so lookups are identical on every
        // platform. `AXINITE_BASE_DIR` is reserved for pointing the merge at
        // this case's temporary directory.
        let snapshot_values: HashMap<String, String> = case
            .snapshot
            .into_iter()
            .filter(|(key, _)| key != "AXINITE_BASE_DIR")
            .collect();
        let secret_values: HashMap<String, String> = case.secrets.into_iter().collect();
        let mut ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", base_dir.clone());
        for (key, value) in &snapshot_values {
            ctx = ctx.with_env(key.clone(), value.clone());
        }
        for (key, value) in &secret_values {
            ctx = ctx.with_secret(key.clone(), value.clone());
        }

        let file_pairs: Vec<(String, String)> = case
            .file
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
