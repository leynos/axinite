//! Tests for the context-driven OpenClaw import command boundary.

use super::{
    ImportCommand, load_import_secrets_crypto, resolve_openclaw_path,
    run_import_command_from_context,
};
use crate::config::EnvContext;

/// Build a dry-run OpenClaw import command.
fn openclaw_command(path: Option<std::path::PathBuf>) -> ImportCommand {
    ImportCommand::Openclaw {
        path,
        dry_run: true,
        re_embed: false,
        user_id: None,
    }
}

/// Build a snapshot that points the import at `home` and a temporary
/// libSQL database, with a valid master key.
fn import_env(home: &std::path::Path, db_dir: &std::path::Path) -> EnvContext {
    EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env(
            "LIBSQL_PATH",
            db_dir.join("axinite.db").to_str().expect("UTF-8 path"),
        )
        .with_env("HOME", home.to_str().expect("UTF-8 path"))
        .with_env("SECRETS_MASTER_KEY", "0123456789abcdef0123456789abcdef")
}

/// Run the command boundary against `env_context`.
async fn run_import(command: &ImportCommand, env_context: &EnvContext) -> anyhow::Result<()> {
    let config = crate::testing::test_utils::test_config_from_context(env_context).await?;
    run_import_command_from_context(command, &config, env_context).await
}

/// Create a temporary home containing a detectable OpenClaw installation.
///
/// Returns the temporary directory (which must stay alive for the test's
/// duration), the installation directory, and a snapshot whose `HOME`
/// points at the temporary home.
fn openclaw_home() -> (tempfile::TempDir, std::path::PathBuf, EnvContext) {
    let home = tempfile::tempdir().expect("temporary home directory should be created");
    let openclaw_dir = home.path().join(".openclaw");
    ambient_fs::create_dir_all(&openclaw_dir).expect("OpenClaw directory should be created");
    ambient_fs::write(openclaw_dir.join("openclaw.json"), "{}")
        .expect("OpenClaw config should be created");
    let env_context =
        EnvContext::default().with_env("HOME", home.path().to_str().expect("UTF-8 path"));
    (home, openclaw_dir, env_context)
}

#[test]
fn explicit_path_wins_over_detection() {
    let (home, _openclaw_dir, env_context) = openclaw_home();
    let explicit = home.path().join("explicit");

    let resolved = resolve_openclaw_path(Some(explicit.clone()), &env_context)
        .expect("an explicit path should resolve");

    assert_eq!(resolved, explicit);
}

#[test]
fn detected_path_uses_the_supplied_snapshot() {
    let (_home, openclaw_dir, env_context) = openclaw_home();

    let resolved = resolve_openclaw_path(None, &env_context).expect("detection should resolve");

    assert_eq!(resolved, openclaw_dir);
}

#[test]
fn missing_installation_falls_back_to_home() {
    let home = tempfile::tempdir().expect("temporary home directory should be created");
    let env_context =
        EnvContext::default().with_env("HOME", home.path().to_str().expect("UTF-8 path"));

    let resolved = resolve_openclaw_path(None, &env_context).expect("fallback should resolve");

    assert_eq!(resolved, home.path().join(".openclaw"));
}

#[test]
fn detection_errors_are_reported() {
    let home = tempfile::tempdir().expect("temporary home directory should be created");
    let not_a_directory = home.path().join("home-file");
    ambient_fs::write(&not_a_directory, "not a directory").expect("home file should be created");
    let env_context =
        EnvContext::default().with_env("HOME", not_a_directory.to_str().expect("UTF-8 path"));

    let error = resolve_openclaw_path(None, &env_context)
        .expect_err("an unreadable candidate path must surface as an error");

    assert!(
        error.to_string().contains("Failed to detect OpenClaw"),
        "{error}"
    );
}

#[tokio::test]
async fn master_key_from_snapshot_is_used() {
    let env_context =
        EnvContext::default().with_env("SECRETS_MASTER_KEY", "0123456789abcdef0123456789abcdef");

    let crypto = load_import_secrets_crypto(&env_context)
        .await
        .expect("a valid snapshot key should be accepted");

    assert!(crypto.encrypt(b"value").is_ok());
}

#[tokio::test]
async fn invalid_master_key_from_snapshot_is_rejected() {
    let env_context = EnvContext::default().with_env("SECRETS_MASTER_KEY", "too-short");

    let error = load_import_secrets_crypto(&env_context)
        .await
        .expect_err("a short master key must be rejected");

    assert!(
        error.to_string().contains("Failed to initialize secrets"),
        "{error}"
    );
}

/// The command boundary must import from the installation detected through
/// the supplied snapshot.
#[tokio::test]
async fn run_import_command_uses_the_detected_installation() -> anyhow::Result<()> {
    let (home, _openclaw_dir, _) = openclaw_home();
    let db_dir = tempfile::tempdir()?;
    let env_context = import_env(home.path(), db_dir.path());

    run_import(&openclaw_command(None), &env_context).await?;
    Ok(())
}

/// An explicit `--path` must win over detection at the command boundary.
#[tokio::test]
async fn run_import_command_uses_an_explicit_path() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let (_install_home, openclaw_dir, _) = openclaw_home();
    let db_dir = tempfile::tempdir()?;
    let env_context = import_env(home.path(), db_dir.path());

    run_import(&openclaw_command(Some(openclaw_dir)), &env_context).await?;
    Ok(())
}

/// With no installation and no explicit path, the command must fall back
/// to `$HOME/.openclaw` and report that it is missing.
#[tokio::test]
async fn run_import_command_falls_back_to_home() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let db_dir = tempfile::tempdir()?;
    let env_context = import_env(home.path(), db_dir.path());

    let error = run_import(&openclaw_command(None), &env_context)
        .await
        .expect_err("a missing installation must be reported");

    let message = error.to_string();
    assert!(
        message.contains(&home.path().join(".openclaw").display().to_string()),
        "the fallback path must be reported: {message}"
    );
    assert!(message.contains("Directory does not exist"), "{message}");
    Ok(())
}

/// An invalid master key in the snapshot must fail the command rather than
/// silently falling back to the keychain.
#[tokio::test]
async fn run_import_command_rejects_an_invalid_master_key() -> anyhow::Result<()> {
    let (home, _openclaw_dir, _) = openclaw_home();
    let db_dir = tempfile::tempdir()?;
    let env_context =
        import_env(home.path(), db_dir.path()).with_env("SECRETS_MASTER_KEY", "too-short");

    let error = run_import(&openclaw_command(None), &env_context)
        .await
        .expect_err("an invalid master key must fail the import");

    assert!(
        error.to_string().contains("SECRETS_MASTER_KEY"),
        "an invalid snapshot key must be reported: {error}"
    );
    Ok(())
}
