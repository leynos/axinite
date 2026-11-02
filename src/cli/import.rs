//! Import command for migrating data from other AI systems.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;

#[cfg(feature = "import")]
use crate::config::EnvContext;
#[cfg(feature = "import")]
use crate::import::ImportOptions;
#[cfg(feature = "import")]
use crate::import::openclaw::OpenClawImporter;

/// Import data from other AI systems.
#[derive(Subcommand, Debug, Clone)]
pub enum ImportCommand {
    /// Import from OpenClaw (memory, history, settings, credentials)
    #[cfg(feature = "import")]
    Openclaw {
        /// Path to OpenClaw directory (default: ~/.openclaw)
        #[arg(long)]
        path: Option<PathBuf>,

        /// Dry-run mode: show what would be imported without writing
        #[arg(long)]
        dry_run: bool,

        /// Re-embed memory if dimensions don't match target provider
        #[arg(long)]
        re_embed: bool,

        /// User ID for imported data (default: 'default')
        #[arg(long)]
        user_id: Option<String>,
    },
}

/// Run an import command.
#[cfg(feature = "import")]
pub async fn run_import_command(
    cmd: &ImportCommand,
    config: &crate::config::Config,
) -> anyhow::Result<()> {
    let env_context = EnvContext::capture_ambient();
    match cmd {
        ImportCommand::Openclaw {
            path,
            dry_run,
            re_embed,
            user_id,
        } => {
            run_import_openclaw(
                config,
                OpenClawImportRequest {
                    path: path.clone(),
                    dry_run: *dry_run,
                    re_embed: *re_embed,
                    user_id: user_id.clone(),
                    env_context: &env_context,
                },
            )
            .await
        }
    }
}

/// Inputs for one OpenClaw import run.
#[cfg(feature = "import")]
struct OpenClawImportRequest<'a> {
    path: Option<PathBuf>,
    dry_run: bool,
    re_embed: bool,
    user_id: Option<String>,
    env_context: &'a EnvContext,
}

/// Run the OpenClaw import.
#[cfg(feature = "import")]
async fn run_import_openclaw(
    config: &crate::config::Config,
    request: OpenClawImportRequest<'_>,
) -> anyhow::Result<()> {
    let OpenClawImportRequest {
        path,
        dry_run,
        re_embed,
        user_id,
        env_context,
    } = request;
    let openclaw_path = resolve_openclaw_path(path, env_context)?;

    let user_id = user_id.unwrap_or_else(|| "default".to_string());

    println!("🔍 OpenClaw Import");
    println!("  Path: {}", openclaw_path.display());
    println!("  User: {}", user_id);
    if dry_run {
        println!("  Mode: DRY RUN (no data will be written)");
    }
    println!();

    // Initialize database
    let db = crate::db::connect_from_config(&config.database)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize database: {}", e))?;

    // Initialize secrets store with master key from env or keychain
    let secrets_crypto = load_import_secrets_crypto(env_context).await?;

    let secrets: Arc<dyn crate::secrets::SecretsStore> = Arc::new(
        crate::secrets::InMemorySecretsStore::new(secrets_crypto.clone()),
    );

    // Initialize workspace
    let workspace = crate::workspace::Workspace::new_with_db(user_id.clone(), db.clone());

    let opts = ImportOptions {
        openclaw_path,
        dry_run,
        re_embed,
        user_id,
    };

    let importer = OpenClawImporter::new(db, workspace, secrets, opts);
    let stats = importer.import().await?;

    // Print results
    println!("Import Complete");
    println!();
    println!("Summary:");
    println!("  Documents:    {}", stats.documents);
    println!("  Chunks:       {}", stats.chunks);
    println!("  Conversations: {}", stats.conversations);
    println!("  Messages:     {}", stats.messages);
    println!("  Settings:     {}", stats.settings);
    println!("  Secrets:      {}", stats.secrets);
    if stats.skipped > 0 {
        println!("  Skipped:      {}", stats.skipped);
    }
    if stats.re_embed_queued > 0 {
        println!("  Re-embed queued: {}", stats.re_embed_queued);
    }
    println!();
    println!("Total imported: {}", stats.total_imported());

    if dry_run {
        println!();
        println!("[DRY RUN] No data was written.");
    }

    Ok(())
}

/// Resolve the OpenClaw directory to import from.
///
/// An explicit `--path` wins. Otherwise the supplied snapshot is used to
/// detect an installation, falling back to `$HOME/.openclaw`. Detection
/// errors are propagated rather than silently treated as "not installed".
#[cfg(feature = "import")]
fn resolve_openclaw_path(
    path: Option<PathBuf>,
    env_context: &EnvContext,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = path {
        return Ok(path);
    }
    if let Some(detected) = OpenClawImporter::detect_from(env_context)
        .map_err(|e| anyhow::anyhow!("Failed to detect OpenClaw installation: {e}"))?
    {
        return Ok(detected);
    }
    Ok(PathBuf::from(env_context.get("HOME").unwrap_or(".")).join(".openclaw"))
}

#[cfg(feature = "import")]
async fn load_import_secrets_crypto(
    env_context: &EnvContext,
) -> anyhow::Result<Arc<crate::secrets::SecretsCrypto>> {
    use secrecy::SecretString;

    let key_hex = match env_context.get_owned("SECRETS_MASTER_KEY") {
        Some(key) => key,
        None => crate::secrets::keychain::get_master_key()
            .await
            .map(|bytes| bytes.iter().map(|byte| format!("{byte:02x}")).collect())
            .map_err(|_| {
                anyhow::anyhow!(
                    "No secrets master key found. Set SECRETS_MASTER_KEY env var or run 'axinite onboard' first."
                )
            })?,
    };
    let crypto = crate::secrets::SecretsCrypto::new(SecretString::from(key_hex))
        .map_err(|e| anyhow::anyhow!("Failed to initialize secrets: {}", e))?;
    Ok(Arc::new(crypto))
}

#[cfg(not(feature = "import"))]
pub async fn run_import_command(
    _cmd: &ImportCommand,
    _config: &crate::config::Config,
) -> anyhow::Result<()> {
    anyhow::bail!("Import feature not enabled. Compile with --features import")
}

#[cfg(all(test, feature = "import"))]
mod tests {
    //! Tests for the context-driven OpenClaw import command boundary.

    use super::{load_import_secrets_crypto, resolve_openclaw_path};
    use crate::config::EnvContext;

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
        ambient_fs::write(&not_a_directory, "not a directory")
            .expect("home file should be created");
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
        let env_context = EnvContext::default()
            .with_env("SECRETS_MASTER_KEY", "0123456789abcdef0123456789abcdef");

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
}
