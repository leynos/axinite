//! Step 7: extensions (tools) installation from the registry.

use super::channel_catalog::{discover_installed_tools, load_registry_catalog};
use super::*;

use crate::registry::manifest::ExtensionManifest;

impl SetupWizard {
    /// Step 7: Extensions (tools) installation from registry.
    pub(super) async fn step_extensions(&mut self) -> Result<(), SetupError> {
        let Some(catalog) = load_registry_catalog() else {
            print_info("Extension registry not found. Skipping tool installation.");
            print_info("Install tools manually with: ironclaw tool install <path>");
            return Ok(());
        };

        let tools: Vec<ExtensionManifest> = catalog
            .list(Some(crate::registry::manifest::ManifestKind::Tool), None)
            .into_iter()
            .cloned()
            .collect();

        if tools.is_empty() {
            print_info("No tools found in registry.");
            return Ok(());
        }

        print_info("Available tools from the extension registry:");
        print_info("Select which tools to install. You can install more later with:");
        print_info("  ironclaw registry install <name>");
        println!();

        // Check which tools are already installed
        let tools_dir = ironclaw_base_dir().join("tools");
        let installed_tools = discover_installed_tools(&tools_dir).await;

        let selected = prompt_tool_selection(&tools, &installed_tools)?;
        if selected.is_empty() {
            print_info("No tools selected.");
            return Ok(());
        }

        // Install selected tools that aren't already on disk
        let repo_root = catalog.root().parent().unwrap_or(catalog.root());
        let installer = crate::registry::installer::RegistryInstaller::new(
            repo_root.to_path_buf(),
            tools_dir.clone(),
            ironclaw_base_dir().join("channels"),
        );

        let (installed_count, auth_needed) =
            install_selected_tools(&installer, &tools, &selected, &installed_tools).await;

        report_install_results(installed_count, &auth_needed);
        Ok(())
    }
}

/// Present the tool selection menu, pre-checking "default"-tagged and
/// already installed tools.
fn prompt_tool_selection(
    tools: &[ExtensionManifest],
    installed_tools: &HashSet<String>,
) -> Result<Vec<usize>, SetupError> {
    let options: Vec<(String, bool)> = tools
        .iter()
        .map(|tool| tool_menu_entry(tool, installed_tools))
        .collect();

    let options_refs: Vec<(&str, bool)> = options.iter().map(|(s, b)| (s.as_str(), *b)).collect();

    select_many("Which tools do you want to install?", &options_refs).map_err(SetupError::Io)
}

/// Build one menu entry (label, pre-checked) for a registry tool.
fn tool_menu_entry(tool: &ExtensionManifest, installed_tools: &HashSet<String>) -> (String, bool) {
    let is_installed = installed_tools.contains(&tool.name);
    let is_default = tool.tags.contains(&"default".to_string());
    let status = if is_installed { " (installed)" } else { "" };
    let auth_hint = tool
        .auth_summary
        .as_ref()
        .and_then(|a| a.method.as_deref())
        .map(|m| format!(" [{}]", m))
        .unwrap_or_default();

    let label = format!(
        "{}{}{} - {}",
        tool.display_name, auth_hint, status, tool.description
    );
    (label, is_default || is_installed)
}

/// Install each selected tool that isn't already on disk.
///
/// Returns the number of tools installed plus the post-setup authentication
/// hints to display.
async fn install_selected_tools(
    installer: &crate::registry::installer::RegistryInstaller,
    tools: &[ExtensionManifest],
    selected: &[usize],
    installed_tools: &HashSet<String>,
) -> (usize, Vec<String>) {
    let mut installed_count = 0;
    let mut auth_needed: Vec<String> = Vec::new();

    for idx in selected {
        let tool = &tools[*idx];
        if installed_tools.contains(&tool.name) {
            continue; // Already installed, skip
        }

        if install_one_tool(installer, tool, &mut auth_needed).await {
            installed_count += 1;
        }
    }

    (installed_count, auth_needed)
}

/// Install a single tool, reporting the outcome and recording any
/// post-setup authentication hint. Returns `true` on success.
async fn install_one_tool(
    installer: &crate::registry::installer::RegistryInstaller,
    tool: &ExtensionManifest,
    auth_needed: &mut Vec<String>,
) -> bool {
    let outcome = match installer.install_with_source_fallback(tool, false).await {
        Ok(outcome) => outcome,
        Err(e) => {
            print_error(&format!("Failed to install {}: {}", tool.display_name, e));
            return false;
        }
    };

    print_success(&format!("Installed {}", outcome.name));
    for warning in &outcome.warnings {
        print_info(&format!("{}: {}", outcome.name, warning));
    }
    record_auth_hint(auth_needed, tool);
    true
}

/// Record an authentication hint for a tool that needs post-setup auth,
/// deduplicating by provider (Google tools share auth).
fn record_auth_hint(auth_needed: &mut Vec<String>, tool: &ExtensionManifest) {
    let Some(auth) = &tool.auth_summary else {
        return;
    };
    if !requires_auth(auth) {
        return;
    }

    let provider = auth.provider.as_deref().unwrap_or(&tool.name);
    let prefix = format!("  {} -", provider);
    if auth_needed.iter().any(|h| h.starts_with(&prefix)) {
        return;
    }
    auth_needed.push(format!("  {} - ironclaw tool auth {}", provider, tool.name));
}

/// Print the install count and any pending authentication hints.
fn report_install_results(installed_count: usize, auth_needed: &[String]) {
    if installed_count > 0 {
        println!();
        print_success(&format!("{} tool(s) installed.", installed_count));
    }

    if !auth_needed.is_empty() {
        println!();
        print_info("Some tools need authentication. Run after setup:");
        for hint in auth_needed {
            print_info(hint);
        }
    }
}
