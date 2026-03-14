//! Tool management CLI commands.
//!
//! Commands for installing, listing, removing, and authenticating WASM tools.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;

use crate::bootstrap::ironclaw_base_dir;
use crate::secrets::SecretsStore;

mod auth;
mod install;
mod listing;
mod printing;
mod setup;

/// Default tools directory.
fn default_tools_dir() -> PathBuf {
    ironclaw_base_dir().join("tools")
}

#[derive(Subcommand, Debug, Clone)]
pub enum ToolCommand {
    /// Install a WASM tool from source directory or .wasm file
    Install {
        /// Path to tool source directory (with Cargo.toml) or .wasm file
        path: PathBuf,

        /// Tool name (defaults to directory/file name)
        #[arg(short, long)]
        name: Option<String>,

        /// Path to capabilities JSON file (auto-detected if not specified)
        #[arg(long)]
        capabilities: Option<PathBuf>,

        /// Target directory for installation (default: ~/.ironclaw/tools/)
        #[arg(short, long)]
        target: Option<PathBuf>,

        /// Build in release mode (default: true)
        #[arg(long, default_value = "true")]
        release: bool,

        /// Skip compilation (use existing .wasm file)
        #[arg(long)]
        skip_build: bool,

        /// Force overwrite if tool already exists
        #[arg(short, long)]
        force: bool,
    },

    /// List installed tools
    List {
        /// Directory to list tools from (default: ~/.ironclaw/tools/)
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Remove an installed tool
    Remove {
        /// Name of the tool to remove
        name: String,

        /// Directory to remove tool from (default: ~/.ironclaw/tools/)
        #[arg(short, long)]
        dir: Option<PathBuf>,
    },

    /// Show information about a tool
    Info {
        /// Name of the tool or path to .wasm file
        name_or_path: String,

        /// Directory to look for tool (default: ~/.ironclaw/tools/)
        #[arg(short, long)]
        dir: Option<PathBuf>,
    },

    /// Configure authentication for a tool
    Auth {
        /// Name of the tool
        name: String,

        /// Directory to look for tool (default: ~/.ironclaw/tools/)
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// User ID for storing the secret (default: "default")
        #[arg(short, long, default_value = "default")]
        user: String,
    },

    /// Configure required secrets for a tool (from setup.required_secrets)
    Setup {
        /// Name of the tool
        name: String,

        /// Directory to look for tool (default: ~/.ironclaw/tools/)
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// User ID for storing the secret (default: "default")
        #[arg(short, long, default_value = "default")]
        user: String,
    },
}

/// Run a tool command.
pub async fn run_tool_command(cmd: ToolCommand) -> anyhow::Result<()> {
    match cmd {
        ToolCommand::Install {
            path,
            name,
            capabilities,
            target,
            release,
            skip_build,
            force,
        } => {
            install::install_tool(path, name, capabilities, target, release, skip_build, force)
                .await
        }
        ToolCommand::List { dir, verbose } => listing::list_tools(dir, verbose).await,
        ToolCommand::Remove { name, dir } => listing::remove_tool(name, dir).await,
        ToolCommand::Info { name_or_path, dir } => listing::show_tool_info(name_or_path, dir).await,
        ToolCommand::Auth { name, dir, user } => auth::auth_tool(name, dir, user).await,
        ToolCommand::Setup { name, dir, user } => setup::setup_tool(name, dir, user).await,
    }
}

/// Initialize the secrets store from environment config.
async fn init_secrets_store() -> anyhow::Result<Arc<dyn SecretsStore + Send + Sync>> {
    crate::cli::init_secrets_store().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(listing::format_size(500), "500 B");
        assert_eq!(listing::format_size(1024), "1.0 KB");
        assert_eq!(listing::format_size(1536), "1.5 KB");
        assert_eq!(listing::format_size(1048576), "1.0 MB");
        assert_eq!(listing::format_size(2621440), "2.5 MB");
    }

    #[test]
    fn test_default_tools_dir() {
        let dir = default_tools_dir();
        assert!(dir.to_string_lossy().contains(".ironclaw"));
        assert!(dir.to_string_lossy().contains("tools"));
    }
}
