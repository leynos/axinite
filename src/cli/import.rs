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
    let openclaw_path = resolve_openclaw_path(path, env_context);

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

#[cfg(feature = "import")]
fn resolve_openclaw_path(path: Option<PathBuf>, env_context: &EnvContext) -> PathBuf {
    path.or_else(OpenClawImporter::detect)
        .unwrap_or_else(|| PathBuf::from(env_context.get("HOME").unwrap_or(".")).join(".openclaw"))
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
