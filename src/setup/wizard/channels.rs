//! Step 6: channel configuration.

use super::channel_catalogue::{
    build_channel_options, discover_wasm_channels, install_selected_bundled_channels,
    install_selected_registry_channels,
};
use super::*;

/// Number of fixed (non-WASM) entries at the top of the channel menu:
/// CLI/TUI, HTTP webhook, and Signal.
const NON_WASM_CHANNEL_COUNT: usize = 3;

impl SetupWizard {
    /// Step 6: Channel configuration.
    pub(super) async fn step_channels(&mut self) -> Result<(), SetupError> {
        // First, configure tunnel (shared across all channels that need webhooks)
        self.configure_tunnel().await;
        println!();

        // Discover available WASM channels
        let channels_dir = axinite_base_dir().join("channels");

        let mut discovered_channels = discover_wasm_channels(&channels_dir).await;
        let installed_names: HashSet<String> = discovered_channels
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        // Build channel list from registry (if available) + bundled + discovered
        let wasm_channel_names = build_channel_options(&discovered_channels);

        let selected = self.prompt_channel_selection(&wasm_channel_names, &installed_names)?;
        let selected_wasm_channels = selected_wasm_names(&wasm_channel_names, &selected);

        // Install selected channels that aren't already on disk, then
        // re-discover so newly installed capabilities files are picked up.
        let any_installed =
            install_selected_channels(&channels_dir, &selected_wasm_channels, &installed_names)
                .await?;
        if any_installed {
            discovered_channels = discover_wasm_channels(&channels_dir).await;
        }

        // Determine if we need secrets context
        let needs_secrets =
            selected.contains(&CHANNEL_INDEX_HTTP) || !selected_wasm_channels.is_empty();
        let secrets = self.init_optional_secrets(needs_secrets).await;

        self.configure_http_channel(selected.contains(&CHANNEL_INDEX_HTTP), secrets.as_ref())
            .await?;
        self.configure_signal_channel(selected.contains(&CHANNEL_INDEX_SIGNAL))
            .await?;

        let discovered_by_name: HashMap<String, ChannelCapabilitiesFile> =
            discovered_channels.into_iter().collect();
        self.configure_wasm_channels(
            selected_wasm_channels,
            &discovered_by_name,
            secrets.as_ref(),
        )
        .await
    }

    /// Configure the shared webhook tunnel, downgrading failures to an
    /// informational message.
    async fn configure_tunnel(&mut self) {
        match setup_tunnel(&self.settings).await {
            Ok(tunnel_settings) => {
                self.settings.tunnel = tunnel_settings;
            }
            Err(e) => {
                print_info(&format!("Tunnel setup skipped: {}", e));
            }
        }
    }

    /// Present the channel selection menu and return the selected indices.
    fn prompt_channel_selection(
        &self,
        wasm_channel_names: &[String],
        installed_names: &HashSet<String>,
    ) -> Result<Vec<usize>, SetupError> {
        let mut options: Vec<(String, bool)> = vec![
            ("CLI/TUI (always enabled)".to_string(), true),
            (
                "HTTP webhook".to_string(),
                self.settings.channels.http_enabled,
            ),
            ("Signal".to_string(), self.settings.channels.signal_enabled),
        ];

        // Add available WASM channels (installed + bundled + registry)
        for name in wasm_channel_names {
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

        select_many("Which channels do you want to enable?", &options_refs).map_err(SetupError::Io)
    }

    /// Initialize the secrets context when channel setup needs it, reporting
    /// (but tolerating) unavailability.
    async fn init_optional_secrets(&mut self, needs_secrets: bool) -> Option<SecretsContext> {
        if !needs_secrets {
            return None;
        }
        match self.init_secrets_context().await {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                print_info(&format!("Secrets not available: {}", e));
                print_info("Channel tokens must be set via environment variables.");
                None
            }
        }
    }

    /// Enable or disable the HTTP webhook channel based on the selection.
    async fn configure_http_channel(
        &mut self,
        enabled: bool,
        secrets: Option<&SecretsContext>,
    ) -> Result<(), SetupError> {
        if !enabled {
            self.settings.channels.http_enabled = false;
            return Ok(());
        }

        println!();
        if let Some(ctx) = secrets {
            let result = setup_http(ctx).await?;
            self.settings.channels.http_enabled = result.enabled;
            self.settings.channels.http_port = Some(result.port);
        } else {
            self.settings.channels.http_enabled = true;
            self.settings.channels.http_port = Some(8080);
            print_info("HTTP webhook enabled on port 8080 (set HTTP_WEBHOOK_SECRET in env)");
        }
        Ok(())
    }

    /// Enable or disable the Signal channel based on the selection.
    async fn configure_signal_channel(&mut self, enabled: bool) -> Result<(), SetupError> {
        let channels = &mut self.settings.channels;
        if !enabled {
            channels.signal_enabled = false;
            channels.signal_http_url = None;
            channels.signal_account = None;
            channels.signal_allow_from = None;
            channels.signal_allow_from_groups = None;
            channels.signal_dm_policy = None;
            channels.signal_group_policy = None;
            channels.signal_group_allow_from = None;
            return Ok(());
        }

        println!();
        let result = setup_signal(&self.settings).await?;
        let channels = &mut self.settings.channels;
        channels.signal_enabled = result.enabled;
        channels.signal_http_url = Some(result.http_url);
        channels.signal_account = Some(result.account);
        channels.signal_allow_from = Some(result.allow_from);
        channels.signal_allow_from_groups = Some(result.allow_from_groups);
        channels.signal_dm_policy = Some(result.dm_policy);
        channels.signal_group_policy = Some(result.group_policy);
        channels.signal_group_allow_from = Some(result.group_allow_from);
        Ok(())
    }

    /// Run per-channel setup for the selected WASM channels and record the
    /// ones that ended up enabled.
    async fn configure_wasm_channels(
        &mut self,
        selected_wasm_channels: Vec<String>,
        discovered_by_name: &HashMap<String, ChannelCapabilitiesFile>,
        secrets: Option<&SecretsContext>,
    ) -> Result<(), SetupError> {
        let mut enabled_wasm_channels = Vec::new();
        for channel_name in selected_wasm_channels {
            println!();
            let Some(ctx) = secrets else {
                // No secrets context, just enable the channel
                print_info(&format!(
                    "{} enabled (configure tokens via environment)",
                    capitalize_first(&channel_name)
                ));
                enabled_wasm_channels.push(channel_name.clone());
                continue;
            };

            let Some(cap_file) = discovered_by_name.get(&channel_name) else {
                print_info(&format!(
                    "Channel '{}' is selected but not available on disk.",
                    channel_name
                ));
                continue;
            };

            let result = setup_discovered_wasm_channel(ctx, &channel_name, cap_file).await?;
            if result.enabled {
                enabled_wasm_channels.push(result.channel_name);
            }
        }

        self.settings.channels.wasm_channels = enabled_wasm_channels;
        Ok(())
    }
}

/// Map the selected menu indices back to WASM channel names.
fn selected_wasm_names(wasm_channel_names: &[String], selected: &[usize]) -> Vec<String> {
    wasm_channel_names
        .iter()
        .enumerate()
        .filter_map(|(idx, name)| {
            if selected.contains(&(NON_WASM_CHANNEL_COUNT + idx)) {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Install the selected channels that aren't already on disk, trying
/// bundled artefacts first and the registry second.
///
/// Returns `true` when at least one channel was installed.
async fn install_selected_channels(
    channels_dir: &std::path::Path,
    selected_wasm_channels: &[String],
    installed_names: &HashSet<String>,
) -> Result<bool, SetupError> {
    let mut any_installed = false;

    // Try bundled channels first (pre-compiled artefacts from channels-src/)
    if let Some(installed) =
        install_selected_bundled_channels(channels_dir, selected_wasm_channels, installed_names)
            .await?
        && !installed.is_empty()
    {
        print_success(&format!(
            "Installed bundled channels: {}",
            installed.join(", ")
        ));
        any_installed = true;
    }

    let installed_from_registry =
        install_selected_registry_channels(channels_dir, selected_wasm_channels, installed_names)
            .await;

    if !installed_from_registry.is_empty() {
        print_success(&format!(
            "Built from registry: {}",
            installed_from_registry.join(", ")
        ));
        any_installed = true;
    }

    Ok(any_installed)
}

/// Set up one discovered WASM channel, prompting for its required secrets
/// when the capabilities file declares any.
async fn setup_discovered_wasm_channel(
    ctx: &SecretsContext,
    channel_name: &str,
    cap_file: &ChannelCapabilitiesFile,
) -> Result<crate::setup::channels::WasmChannelSetupResult, SetupError> {
    if cap_file.setup.required_secrets.is_empty() {
        print_info(&format!(
            "No setup configuration found for {}",
            channel_name
        ));
        return Ok(crate::setup::channels::WasmChannelSetupResult {
            enabled: true,
            channel_name: channel_name.to_string(),
        });
    }
    Ok(setup_wasm_channel(ctx, channel_name, &cap_file.setup).await?)
}
