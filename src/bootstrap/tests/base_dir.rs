//! Tests for computing the Axinite base directory and related paths.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{EnvContext, dotenv_layers::DotenvLayers};

use super::super::*;

#[test]
fn test_axinite_base_dir_default() {
    let context = EnvContext::for_testing(
        HashMap::from([(String::from("HOME"), String::from("/home/test-user"))]),
        HashMap::new(),
    );
    let path = compute_axinite_base_dir_from(&context);
    assert_eq!(path, PathBuf::from("/home/test-user/.axinite"));
}

#[test]
fn test_axinite_base_dir_env_override() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/custom/axinite/path");
    let path = compute_axinite_base_dir_from(&ctx);
    assert_eq!(path, PathBuf::from("/custom/axinite/path"));
}

#[test]
fn test_compute_base_dir_env_path_join() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/my/custom/dir");
    let base_path = compute_axinite_base_dir_from(&ctx);
    let env_path = base_path.join(".env");
    assert_eq!(env_path, PathBuf::from("/my/custom/dir/.env"));
}

#[test]
fn test_axinite_base_dir_empty_env() {
    let ctx = EnvContext::for_testing(
        HashMap::from([
            (String::from("AXINITE_BASE_DIR"), String::new()),
            (String::from("HOME"), String::from("/home/test-user")),
        ]),
        HashMap::new(),
    );
    let path = compute_axinite_base_dir_from(&ctx);
    assert_eq!(path, PathBuf::from("/home/test-user/.axinite"));
}

#[test]
fn test_axinite_base_dir_special_chars() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/tmp/test_with-special.chars");
    let path = compute_axinite_base_dir_from(&ctx);
    assert_eq!(path, PathBuf::from("/tmp/test_with-special.chars"));
}

#[test]
fn dotenv_layers_fill_missing_context_entries_below_process_values() -> std::io::Result<()> {
    let dir = tempfile::tempdir()?;
    let cwd = dir.path().join("project");
    let base_dir = dir.path().join("axinite");
    ambient_fs::create_dir_all(&cwd)?;
    ambient_fs::create_dir_all(&base_dir)?;
    ambient_fs::write(cwd.join(".env"), "")?;
    ambient_fs::write(
        base_dir.join(".env"),
        "DATABASE_BACKEND=libsql\nLIBSQL_PATH=/saved/axinite.db\n",
    )?;
    let process = HashMap::from([
        (
            String::from("AXINITE_BASE_DIR"),
            base_dir.display().to_string(),
        ),
        (String::from("DATABASE_BACKEND"), String::from("postgres")),
    ]);
    let layers = DotenvLayers::load(&cwd, &process, None);
    let context = EnvContext::from_dotenv_layers(process, HashMap::new(), &layers);

    assert_eq!(context.get("DATABASE_BACKEND"), Some("postgres"));
    assert_eq!(context.get("LIBSQL_PATH"), Some("/saved/axinite.db"));
    Ok(())
}
