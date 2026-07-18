//! Final step: save settings and print the setup summary.

use super::*;

impl SetupWizard {
    /// Save settings to the database and `~/.axinite/.env`, then print summary.
    pub(super) async fn save_and_summarize(&mut self) -> Result<(), SetupError> {
        self.settings.onboard_completed = true;

        // Final persist (idempotent — earlier incremental saves already wrote
        // most settings, but this ensures onboard_completed is saved).
        let saved = self.persist_settings().await?;

        if !saved {
            return Err(SetupError::Database(
                "No database connection, cannot save settings".to_string(),
            ));
        }

        // Write bootstrap env (also idempotent)
        self.write_bootstrap_env()?;

        println!();
        print_success("Configuration saved to database");
        println!();

        // Print summary
        println!("Configuration Summary:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        self.print_database_summary();
        self.print_security_summary();
        self.print_provider_summary();
        self.print_model_summary();
        self.print_embeddings_summary();
        self.print_tunnel_summary();
        self.print_channels_summary();
        self.print_heartbeat_summary();
        self.print_next_steps();

        Ok(())
    }

    /// Summarize the configured database backend.
    fn print_database_summary(&self) {
        let backend = self
            .settings
            .database_backend
            .as_deref()
            .unwrap_or("postgres");
        match backend {
            "libsql" => {
                if let Some(ref path) = self.settings.libsql_path {
                    println!("  Database: libSQL ({})", path);
                } else {
                    println!("  Database: libSQL (default path)");
                }
                if self.settings.libsql_url.is_some() {
                    println!("  Turso sync: enabled");
                }
            }
            _ => {
                if self.settings.database_url.is_some() {
                    println!("  Database: PostgreSQL (configured)");
                }
            }
        }
    }

    /// Summarize where the secrets master key is stored.
    fn print_security_summary(&self) {
        match self.settings.secrets_master_key_source {
            KeySource::Keychain => println!("  Security: OS keychain"),
            KeySource::Env => println!("  Security: environment variable"),
            KeySource::None => println!("  Security: disabled"),
        }
    }

    /// Summarize the configured inference provider.
    fn print_provider_summary(&self) {
        let Some(ref provider) = self.settings.llm_backend else {
            return;
        };
        let display = match provider.as_str() {
            "nearai" => "NEAR AI",
            "anthropic" => "Anthropic",
            "openai" => "OpenAI",
            "ollama" => "Ollama",
            "openai_compatible" => "OpenAI-compatible",
            "bedrock" => "AWS Bedrock",
            other => other,
        };
        println!("  Provider: {}", display);
    }

    /// Summarize the selected model, truncating long names.
    fn print_model_summary(&self) {
        let Some(ref model) = self.settings.selected_model else {
            return;
        };
        // Truncate long model names (char-based to avoid UTF-8 panic)
        let display = if model.chars().count() > 40 {
            let truncated: String = model.chars().take(37).collect();
            format!("{}...", truncated)
        } else {
            model.clone()
        };
        println!("  Model: {}", display);
    }

    /// Summarize the embeddings configuration.
    fn print_embeddings_summary(&self) {
        if self.settings.embeddings.enabled {
            println!(
                "  Embeddings: {} ({})",
                self.settings.embeddings.provider, self.settings.embeddings.model
            );
        } else {
            println!("  Embeddings: disabled");
        }
    }

    /// Summarize the tunnel configuration, when one is set up.
    fn print_tunnel_summary(&self) {
        if let Some(ref tunnel_url) = self.settings.tunnel.public_url {
            println!("  Tunnel: {} (static)", tunnel_url);
        } else if let Some(ref provider) = self.settings.tunnel.provider {
            println!("  Tunnel: {} (managed, starts at boot)", provider);
        }
    }

    /// Summarize the enabled channels.
    fn print_channels_summary(&self) {
        let has_tunnel =
            self.settings.tunnel.public_url.is_some() || self.settings.tunnel.provider.is_some();

        println!("  Channels:");
        println!("    - CLI/TUI: enabled");

        if self.settings.channels.http_enabled {
            let port = self.settings.channels.http_port.unwrap_or(8080);
            println!("    - HTTP: enabled (port {})", port);
        }

        for channel_name in &self.settings.channels.wasm_channels {
            let mode = if has_tunnel { "webhook" } else { "polling" };
            println!(
                "    - {}: enabled ({})",
                capitalize_first(channel_name),
                mode
            );
        }
    }

    /// Summarize the heartbeat configuration, when enabled.
    fn print_heartbeat_summary(&self) {
        if self.settings.heartbeat.enabled {
            println!(
                "  Heartbeat: every {} minutes",
                self.settings.heartbeat.interval_secs / 60
            );
        }
    }

    /// Print the closing instructions after a successful setup.
    fn print_next_steps(&self) {
        println!();
        println!("To start the agent, run:");
        println!("  axinite");
        println!();
        println!("To change settings later:");
        println!("  axinite config set <setting> <value>");
        println!("  axinite onboard");
        println!();

        if self.config.quick {
            print_info(
                "Tip: Run `axinite onboard` to configure channels, extensions, embeddings, and more.",
            );
            println!();
        }
    }
}
