//! Migration of legacy `bootstrap.json` values into the `.env` file.

use std::{borrow::Cow, collections::HashSet, io, path::Path};

use super::rename_to_migrated;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KnownEnvKey {
    DatabaseUrl,
    DatabaseBackend,
    LlmBackend,
    OnboardCompleted,
    EmbeddingEnabled,
}

impl KnownEnvKey {
    const fn as_str(self) -> &'static str {
        match self {
            Self::DatabaseUrl => "DATABASE_URL",
            Self::DatabaseBackend => "DATABASE_BACKEND",
            Self::LlmBackend => "LLM_BACKEND",
            Self::OnboardCompleted => "ONBOARD_COMPLETED",
            Self::EmbeddingEnabled => "EMBEDDING_ENABLED",
        }
    }
}

struct EnvPair<'a> {
    key: KnownEnvKey,
    value: Cow<'a, str>,
}

const _: [KnownEnvKey; 5] = [
    KnownEnvKey::DatabaseUrl,
    KnownEnvKey::DatabaseBackend,
    KnownEnvKey::LlmBackend,
    KnownEnvKey::OnboardCompleted,
    KnownEnvKey::EmbeddingEnabled,
];

/// Reads `bootstrap.json` in the same directory as `env_path` and, if a
/// `database_url` field is present, writes it as `DATABASE_URL` into the
/// `.env` file at `env_path`.
///
/// The function is idempotent: running it a second time after
/// `bootstrap.json` has already been renamed to `.migrated` is a no-op.
///
/// # Arguments
///
/// * `env_path` — Absolute path to the `.env` file that should receive the
///   migrated key. The parent directory is used to locate `bootstrap.json`.
///
/// # Returns
///
/// Returns `()` in all cases. Missing `bootstrap.json`, a missing
/// `database_url` field, and `.env` write errors are handled internally with
/// early returns rather than propagated to the caller. Missing
/// `bootstrap.json` and a missing `database_url` field are silent and do not
/// emit warning-level diagnostics.
///
/// # Errors
///
/// This function does not return an error. The inability to rename
/// `bootstrap.json` to `.migrated` after the env write succeeds is logged by
/// [`rename_to_migrated`] and discarded so that the application can continue
/// to start up.
pub(crate) fn migrate_bootstrap_json_to_env(env_path: &Path) {
    let axinite_dir = env_path.parent().unwrap_or_else(|| Path::new("."));
    let bootstrap_path = axinite_dir.join("bootstrap.json");
    let parsed = match read_bootstrap_json(&bootstrap_path) {
        Ok(Some(parsed)) => parsed,
        Ok(None) | Err(_) => return,
    };
    let pairs = extract_env_pairs(&parsed);
    if pairs.is_empty() {
        return;
    }
    if upsert_env_pairs(env_path, &pairs).is_err() {
        return;
    }
    let _ = rename_to_migrated(&bootstrap_path);
    eprintln!(
        "Migrated DATABASE_URL from bootstrap.json to {}",
        env_path.display()
    );
}

fn read_bootstrap_json(path: &Path) -> io::Result<Option<serde_json::Value>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = ambient_fs::read_to_string(path)?;
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn extract_env_pairs(json: &serde_json::Value) -> Vec<EnvPair<'_>> {
    json.get("database_url")
        .and_then(serde_json::Value::as_str)
        .map(|url| {
            vec![EnvPair {
                key: KnownEnvKey::DatabaseUrl,
                value: Cow::Borrowed(url),
            }]
        })
        .unwrap_or_default()
}

fn quote_env_value(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}

fn upsert_env_pairs(env_path: &Path, pairs: &[EnvPair<'_>]) -> io::Result<()> {
    ensure_parent_directory(env_path)?;
    let keys_being_written = keys_being_written(pairs);
    let existing = read_existing_env(env_path)?;
    let merged = merge_env_contents(&existing, &keys_being_written, pairs);
    write_env_file(env_path, &merged)
}

/// Create the directory that will hold `env_path`, if it has one.
fn ensure_parent_directory(env_path: &Path) -> io::Result<()> {
    let Some(parent) = env_path.parent() else {
        return Ok(());
    };
    ambient_fs::create_dir_all(parent).inspect_err(|error| {
        eprintln!("Warning: failed to create {}: {}", parent.display(), error);
    })
}

/// Collect the keys the migration is about to write.
fn keys_being_written(pairs: &[EnvPair<'_>]) -> HashSet<&'static str> {
    pairs.iter().map(|pair| pair.key.as_str()).collect()
}

/// Read the current `.env` contents, treating a missing file as empty.
fn read_existing_env(env_path: &Path) -> io::Result<String> {
    match ambient_fs::read_to_string(env_path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => {
            eprintln!(
                "Warning: failed to migrate bootstrap.json to .env: {}",
                error
            );
            Err(error)
        }
    }
}

/// Return `existing` with the migrated keys removed, then append the new pairs
/// in their quoted form.
fn merge_env_contents(
    existing: &str,
    keys_being_written: &HashSet<&'static str>,
    pairs: &[EnvPair<'_>],
) -> String {
    let mut result = String::new();
    for line in existing.lines() {
        if !is_overwritten_key(line, keys_being_written) {
            result.push_str(line);
            result.push('\n');
        }
    }

    for pair in pairs {
        result.push_str(&format!(
            "{}=\"{}\"\n",
            pair.key.as_str(),
            quote_env_value(pair.value.as_ref())
        ));
    }
    result
}

/// Report whether `line` assigns one of the keys the migration rewrites.
fn is_overwritten_key(line: &str, keys_being_written: &HashSet<&'static str>) -> bool {
    line.split_once('=')
        .is_some_and(|(key, _)| keys_being_written.contains(key.trim()))
}

/// Write the merged `.env` contents, reporting a write failure.
fn write_env_file(env_path: &Path, contents: &str) -> io::Result<()> {
    ambient_fs::write(env_path, contents).inspect_err(|error| {
        eprintln!(
            "Warning: failed to migrate bootstrap.json to .env: {}",
            error
        );
    })
}
