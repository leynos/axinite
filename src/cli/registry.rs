//! Registry CLI commands for discovering and installing extensions.

mod info_output;

use clap::Subcommand;

use crate::registry::catalog::RegistryCatalog;
use crate::registry::installer::RegistryInstaller;
use crate::registry::manifest::{BundleDefinition, ExtensionManifest, ManifestKind};
use info_output::{print_bundle_info, print_extension_info};

/// Subcommands of `axinite registry`: list, inspect and install extensions and
/// bundles. Dispatched by [`run_registry_command`].
#[derive(Subcommand, Debug, Clone)]
pub enum RegistryCommand {
    /// List available extensions in the registry
    List {
        /// Filter by kind: "tool" or "channel"
        #[arg(short, long)]
        kind: Option<String>,

        /// Filter by tag (e.g. "default", "google", "messaging")
        #[arg(short, long)]
        tag: Option<String>,

        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show detailed information about an extension or bundle
    Info {
        /// Extension or bundle name (e.g. "slack", "google", "tools/gmail")
        name: String,
    },

    /// Install an extension or bundle from the registry
    Install {
        /// Extension or bundle name (e.g. "slack", "google", "default")
        name: String,

        /// Force overwrite if already installed
        #[arg(short, long)]
        force: bool,

        /// Build from source instead of downloading pre-built artefact
        #[arg(long)]
        build: bool,
    },

    /// Install the default bundle of recommended extensions
    InstallDefaults {
        /// Force overwrite if already installed
        #[arg(short, long)]
        force: bool,

        /// Build from source instead of downloading pre-built artefact
        #[arg(long)]
        build: bool,
    },
}

/// Run a registry command.
pub async fn run_registry_command(cmd: RegistryCommand) -> anyhow::Result<()> {
    // For install commands that need to build from source, a disk registry is required.
    // For list/info, embedded manifests suffice.
    let registry_dir = RegistryCatalog::find_dir();
    let catalogue = if let Some(ref dir) = registry_dir {
        RegistryCatalog::load(dir)?
    } else {
        RegistryCatalog::load_or_embedded()?
    };

    // Resolve repo root for installer (empty path when running from binary)
    let repo_root = registry_dir
        .as_ref()
        .and_then(|d| d.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();

    match cmd {
        RegistryCommand::List { kind, tag, verbose } => {
            cmd_list(&catalogue, kind.as_deref(), tag.as_deref(), verbose)
        }
        RegistryCommand::Info { name } => cmd_info(&catalogue, &name),
        RegistryCommand::Install { name, force, build } => {
            cmd_install(&catalogue, &repo_root, &name, force, build).await
        }
        RegistryCommand::InstallDefaults { force, build } => {
            cmd_install(&catalogue, &repo_root, "default", force, build).await
        }
    }
}

fn cmd_list(
    catalogue: &RegistryCatalog,
    kind: Option<&str>,
    tag: Option<&str>,
    verbose: bool,
) -> anyhow::Result<()> {
    let kind_filter = match kind {
        Some("tool" | "tools") => Some(ManifestKind::Tool),
        Some("channel" | "channels") => Some(ManifestKind::Channel),
        Some(other) => anyhow::bail!("Unknown kind '{}'. Use 'tool' or 'channel'.", other),
        None => None,
    };

    let manifests = catalogue.list(kind_filter, tag);

    if manifests.is_empty() {
        println!("No extensions found matching the criteria.");
        return Ok(());
    }

    // Print header
    if verbose {
        println!(
            "{:<20} {:<8} {:<8} {:<10} DESCRIPTION",
            "NAME", "KIND", "VERSION", "AUTH"
        );
        println!("{}", "-".repeat(80));
    } else {
        println!("{:<20} {:<8} DESCRIPTION", "NAME", "KIND");
        println!("{}", "-".repeat(60));
    }

    for m in &manifests {
        if verbose {
            let auth = m
                .auth_summary
                .as_ref()
                .and_then(|a| a.method.as_deref())
                .unwrap_or("none");
            println!(
                "{:<20} {:<8} {:<8} {:<10} {}",
                m.name, m.kind, m.version, auth, m.description
            );
        } else {
            println!("{:<20} {:<8} {}", m.name, m.kind, m.description);
        }
    }

    println!("\n{} extension(s) found.", manifests.len());

    // Show bundles hint
    let bundle_names = catalogue.bundle_names();
    if !bundle_names.is_empty() {
        println!("\nBundles available: {}", bundle_names.join(", "));
        println!("Use `axinite registry info <bundle>` for details.");
    }

    Ok(())
}

fn cmd_info(catalogue: &RegistryCatalog, name: &str) -> anyhow::Result<()> {
    // Check if it's a bundle
    if let Some(bundle) = catalogue.get_bundle(name) {
        print_bundle_info(catalogue, bundle);
        return Ok(());
    }

    // Single extension (use get_strict to surface ambiguous bare names)
    let manifest = catalogue
        .get_strict(name)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    print_extension_info(manifest);
    Ok(())
}

async fn cmd_install(
    catalogue: &RegistryCatalog,
    repo_root: &std::path::Path,
    name: &str,
    force: bool,
    prefer_build: bool,
) -> anyhow::Result<()> {
    let installer = RegistryInstaller::with_defaults(repo_root.to_path_buf());

    let (manifests, bundle) = catalogue.resolve(name)?;

    if manifests.is_empty() {
        anyhow::bail!("No extensions found for '{}'.", name);
    }

    let options = InstallOptions {
        force,
        prefer_build,
    };
    match bundle {
        Some(bundle_def) => {
            install_bundle_command(&installer, &manifests, bundle_def, options).await
        }
        None => install_single_command(&installer, manifests[0], options).await?,
    }

    Ok(())
}

/// Flags shared by the bundle and single-extension install paths.
#[derive(Clone, Copy)]
struct InstallOptions {
    force: bool,
    prefer_build: bool,
}

/// Install every extension of a bundle and print the per-extension results.
async fn install_bundle_command(
    installer: &RegistryInstaller,
    manifests: &[&ExtensionManifest],
    bundle_def: &BundleDefinition,
    options: InstallOptions,
) {
    println!(
        "Installing bundle '{}' ({} extensions)...\n",
        bundle_def.display_name,
        manifests.len()
    );

    let (outcomes, hints) = installer
        .install_bundle(manifests, bundle_def, options.force, options.prefer_build)
        .await;

    println!("\n--- Results ---");
    for outcome in &outcomes {
        let caps_status = if outcome.has_capabilities { "+" } else { "-" };
        println!(
            "  [{}] {} ({}) -> {}",
            caps_status,
            outcome.name,
            outcome.kind,
            outcome.wasm_path.display()
        );
        for w in &outcome.warnings {
            println!("      Warning: {}", w);
        }
    }

    if !hints.is_empty() {
        println!("\nAuth setup:");
        for hint in &hints {
            println!("{}", hint);
        }
    }

    println!(
        "\nInstalled {}/{} extensions.",
        outcomes.len(),
        manifests.len()
    );
}

/// Install one extension, print its outcome, and point to the auth step if needed.
async fn install_single_command(
    installer: &RegistryInstaller,
    manifest: &ExtensionManifest,
    options: InstallOptions,
) -> anyhow::Result<()> {
    let outcome = installer
        .install(manifest, options.force, options.prefer_build)
        .await?;

    println!("\nInstalled successfully:");
    println!("  Name: {}", outcome.name);
    println!("  Kind: {}", outcome.kind);
    println!("  WASM: {}", outcome.wasm_path.display());
    println!("  Capabilities: {}", outcome.has_capabilities);

    if let Some(auth) = &manifest.auth_summary
        && auth.method.as_deref() != Some("none")
    {
        println!(
            "\nNext step: authenticate with `axinite tool auth {}`",
            manifest.name
        );
        if let Some(url) = &auth.setup_url {
            println!("  Setup credentials at: {}", url);
        }
    }
    Ok(())
}
