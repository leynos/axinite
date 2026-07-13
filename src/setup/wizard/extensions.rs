//! Step 7: extensions (tools) installation from the registry.

use super::channel_catalog::{discover_installed_tools, load_registry_catalog};
use super::*;

impl SetupWizard {
    /// Step 7: Extensions (tools) installation from registry.
    pub(super) async fn step_extensions(&mut self) -> Result<(), SetupError> {
        let catalog = match load_registry_catalog() {
            Some(c) => c,
            None => {
                print_info("Extension registry not found. Skipping tool installation.");
                print_info("Install tools manually with: ironclaw tool install <path>");
                return Ok(());
            }
        };

        let tools: Vec<_> = catalog
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

        // Build options: show display_name + description, pre-check "default" tagged + already installed
        let mut options: Vec<(String, bool)> = Vec::new();
        for tool in &tools {
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
            options.push((label, is_default || is_installed));
        }

        let options_refs: Vec<(&str, bool)> =
            options.iter().map(|(s, b)| (s.as_str(), *b)).collect();

        let selected = select_many("Which tools do you want to install?", &options_refs)
            .map_err(SetupError::Io)?;

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

        let mut installed_count = 0;
        let mut auth_needed: Vec<String> = Vec::new();

        for idx in &selected {
            let tool = &tools[*idx];
            if installed_tools.contains(&tool.name) {
                continue; // Already installed, skip
            }

            match installer.install_with_source_fallback(tool, false).await {
                Ok(outcome) => {
                    print_success(&format!("Installed {}", outcome.name));
                    for warning in &outcome.warnings {
                        print_info(&format!("{}: {}", outcome.name, warning));
                    }
                    installed_count += 1;

                    // Track auth needs
                    if let Some(auth) = &tool.auth_summary
                        && requires_auth(auth)
                    {
                        let provider = auth.provider.as_deref().unwrap_or(&tool.name);
                        // Only mention unique providers (Google tools share auth)
                        let hint = format!("  {} - ironclaw tool auth {}", provider, tool.name);
                        if !auth_needed
                            .iter()
                            .any(|h| h.starts_with(&format!("  {} -", provider)))
                        {
                            auth_needed.push(hint);
                        }
                    }
                }
                Err(e) => {
                    print_error(&format!("Failed to install {}: {}", tool.display_name, e));
                }
            }
        }

        if installed_count > 0 {
            println!();
            print_success(&format!("{} tool(s) installed.", installed_count));
        }

        if !auth_needed.is_empty() {
            println!();
            print_info("Some tools need authentication. Run after setup:");
            for hint in &auth_needed {
                print_info(hint);
            }
        }

        Ok(())
    }
}
