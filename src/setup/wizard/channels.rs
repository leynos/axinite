//! Step 6: channel configuration.

use super::channel_catalog::{
    build_channel_options, discover_wasm_channels, install_selected_bundled_channels,
    install_selected_registry_channels,
};
use super::*;

impl SetupWizard {
    /// Step 6: Channel configuration.
    pub(super) async fn step_channels(&mut self) -> Result<(), SetupError> {
        // First, configure tunnel (shared across all channels that need webhooks)
        match setup_tunnel(&self.settings).await {
            Ok(tunnel_settings) => {
                self.settings.tunnel = tunnel_settings;
            }
            Err(e) => {
                print_info(&format!("Tunnel setup skipped: {}", e));
            }
        }
        println!();

        // Discover available WASM channels
        let channels_dir = ironclaw_base_dir().join("channels");

        let mut discovered_channels = discover_wasm_channels(&channels_dir).await;
        let installed_names: HashSet<String> = discovered_channels
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        // Build channel list from registry (if available) + bundled + discovered
        let wasm_channel_names = build_channel_options(&discovered_channels);

        // Build options list dynamically
        let mut options: Vec<(String, bool)> = vec![
            ("CLI/TUI (always enabled)".to_string(), true),
            (
                "HTTP webhook".to_string(),
                self.settings.channels.http_enabled,
            ),
            ("Signal".to_string(), self.settings.channels.signal_enabled),
        ];

        let non_wasm_count = options.len();

        // Add available WASM channels (installed + bundled + registry)
        for name in &wasm_channel_names {
            let is_enabled = self.settings.channels.wasm_channels.contains(name);
            let label = if installed_names.contains(name) {
                format!("{} (installed)", capitalize_first(name))
            } else {
                format!("{} (will install)", capitalize_first(name))
            };
            options.push((label, is_enabled));
        }

        let options_refs: Vec<(&str, bool)> =
            options.iter().map(|(s, b)| (s.as_str(), *b)).collect();

        let selected = select_many("Which channels do you want to enable?", &options_refs)
            .map_err(SetupError::Io)?;

        let selected_wasm_channels: Vec<String> = wasm_channel_names
            .iter()
            .enumerate()
            .filter_map(|(idx, name)| {
                if selected.contains(&(non_wasm_count + idx)) {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();

        // Install selected channels that aren't already on disk
        let mut any_installed = false;

        // Try bundled channels first (pre-compiled artifacts from channels-src/)
        if let Some(installed) = install_selected_bundled_channels(
            &channels_dir,
            &selected_wasm_channels,
            &installed_names,
        )
        .await?
            && !installed.is_empty()
        {
            print_success(&format!(
                "Installed bundled channels: {}",
                installed.join(", ")
            ));
            any_installed = true;
        }

        let installed_from_registry = install_selected_registry_channels(
            &channels_dir,
            &selected_wasm_channels,
            &installed_names,
        )
        .await;

        if !installed_from_registry.is_empty() {
            print_success(&format!(
                "Built from registry: {}",
                installed_from_registry.join(", ")
            ));
            any_installed = true;
        }

        // Re-discover after installs
        if any_installed {
            discovered_channels = discover_wasm_channels(&channels_dir).await;
        }

        // Determine if we need secrets context
        let needs_secrets =
            selected.contains(&CHANNEL_INDEX_HTTP) || !selected_wasm_channels.is_empty();
        let secrets = if needs_secrets {
            match self.init_secrets_context().await {
                Ok(ctx) => Some(ctx),
                Err(e) => {
                    print_info(&format!("Secrets not available: {}", e));
                    print_info("Channel tokens must be set via environment variables.");
                    None
                }
            }
        } else {
            None
        };

        // HTTP channel
        if selected.contains(&CHANNEL_INDEX_HTTP) {
            println!();
            if let Some(ref ctx) = secrets {
                let result = setup_http(ctx).await?;
                self.settings.channels.http_enabled = result.enabled;
                self.settings.channels.http_port = Some(result.port);
            } else {
                self.settings.channels.http_enabled = true;
                self.settings.channels.http_port = Some(8080);
                print_info("HTTP webhook enabled on port 8080 (set HTTP_WEBHOOK_SECRET in env)");
            }
        } else {
            self.settings.channels.http_enabled = false;
        }

        // Signal channel
        if selected.contains(&CHANNEL_INDEX_SIGNAL) {
            println!();
            let result = setup_signal(&self.settings).await?;
            self.settings.channels.signal_enabled = result.enabled;
            self.settings.channels.signal_http_url = Some(result.http_url);
            self.settings.channels.signal_account = Some(result.account);
            self.settings.channels.signal_allow_from = Some(result.allow_from);
            self.settings.channels.signal_allow_from_groups = Some(result.allow_from_groups);
            self.settings.channels.signal_dm_policy = Some(result.dm_policy);
            self.settings.channels.signal_group_policy = Some(result.group_policy);
            self.settings.channels.signal_group_allow_from = Some(result.group_allow_from);
        } else {
            self.settings.channels.signal_enabled = false;
            self.settings.channels.signal_http_url = None;
            self.settings.channels.signal_account = None;
            self.settings.channels.signal_allow_from = None;
            self.settings.channels.signal_allow_from_groups = None;
            self.settings.channels.signal_dm_policy = None;
            self.settings.channels.signal_group_policy = None;
            self.settings.channels.signal_group_allow_from = None;
        }

        let discovered_by_name: HashMap<String, ChannelCapabilitiesFile> =
            discovered_channels.into_iter().collect();

        // Process selected WASM channels
        let mut enabled_wasm_channels = Vec::new();
        for channel_name in selected_wasm_channels {
            println!();
            if let Some(ref ctx) = secrets {
                let result = if let Some(cap_file) = discovered_by_name.get(&channel_name) {
                    if !cap_file.setup.required_secrets.is_empty() {
                        setup_wasm_channel(ctx, &channel_name, &cap_file.setup).await?
                    } else {
                        print_info(&format!(
                            "No setup configuration found for {}",
                            channel_name
                        ));
                        crate::setup::channels::WasmChannelSetupResult {
                            enabled: true,
                            channel_name: channel_name.clone(),
                        }
                    }
                } else {
                    print_info(&format!(
                        "Channel '{}' is selected but not available on disk.",
                        channel_name
                    ));
                    continue;
                };

                if result.enabled {
                    enabled_wasm_channels.push(result.channel_name);
                }
            } else {
                // No secrets context, just enable the channel
                print_info(&format!(
                    "{} enabled (configure tokens via environment)",
                    capitalize_first(&channel_name)
                ));
                enabled_wasm_channels.push(channel_name.clone());
            }
        }

        self.settings.channels.wasm_channels = enabled_wasm_channels;

        Ok(())
    }
}
