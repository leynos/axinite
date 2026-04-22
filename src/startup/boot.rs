//! Startup boot-screen rendering and tests.

use ironclaw::{cli::Cli, config::Config};

/// Runtime-computed values used to populate the startup boot screen.
pub(crate) struct BootScreenContext<'a> {
    /// Primary LLM model identifier shown on the boot screen.
    pub(crate) llm_model: String,
    /// Optional cheaper/faster LLM model, shown when configured.
    pub(crate) cheap_model: Option<String>,
    /// Total number of registered tools available to the agent.
    pub(crate) tool_count: usize,
    /// Optional gateway web-UI URL, shown when the gateway channel is active.
    pub(crate) gateway_url: Option<String>,
    /// Current Docker/sandbox availability status.
    pub(crate) docker_status: ironclaw::sandbox::DockerStatus,
    /// Names of all enabled channels, displayed in the boot banner.
    pub(crate) channel_names: Vec<String>,
    /// Active tunnel handle used to prefer a public tunnel URL over the
    /// configured static one.
    pub(crate) active_tunnel: &'a Option<Box<dyn ironclaw::tunnel::Tunnel>>,
}

/// Renders and prints the startup boot screen to stdout.
///
/// Returns immediately when CLI channels are disabled or when a one-shot
/// `--message` flag is present on `cli`, since neither scenario presents an
/// interactive session.
pub(crate) fn print_startup_info(config: &Config, cli: &Cli, data: &BootScreenContext<'_>) {
    let Some(boot_info) = build_boot_info(config, cli, data) else {
        return;
    };
    ironclaw::boot_screen::print_boot_screen(&boot_info);
}

/// Constructs the `BootInfo` value passed to
/// `ironclaw::boot_screen::print_boot_screen`.
fn build_boot_info(
    config: &Config,
    cli: &Cli,
    data: &BootScreenContext<'_>,
) -> Option<ironclaw::boot_screen::BootInfo> {
    if !config.channels.cli.enabled || cli.message.is_some() {
        return None;
    }
    Some(ironclaw::boot_screen::BootInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        agent_name: config.agent.name.clone(),
        llm_backend: config.llm.backend.to_string(),
        llm_model: data.llm_model.clone(),
        cheap_model: data.cheap_model.clone(),
        db_backend: if cli.no_db {
            "none".to_string()
        } else {
            config.database.backend.to_string()
        },
        db_connected: !cli.no_db,
        tool_count: data.tool_count,
        gateway_url: data
            .gateway_url
            .as_deref()
            .map(crate::startup::url_sanitize::sanitize_display_url),
        embeddings_enabled: config.embeddings.enabled,
        embeddings_provider: config
            .embeddings
            .enabled
            .then(|| config.embeddings.provider.clone()),
        heartbeat_enabled: config.heartbeat.enabled,
        heartbeat_interval_secs: config.heartbeat.interval_secs,
        sandbox_enabled: config.sandbox.enabled,
        docker_status: data.docker_status,
        claude_code_enabled: config.claude_code.enabled,
        routines_enabled: config.routines.enabled,
        skills_enabled: config.skills.enabled,
        channels: data.channel_names.clone(),
        tunnel_url: data
            .active_tunnel
            .as_ref()
            .and_then(|t| t.public_url())
            .or_else(|| config.tunnel.public_url.clone())
            .as_deref()
            .map(crate::startup::url_sanitize::sanitize_display_url),
        tunnel_provider: data.active_tunnel.as_ref().map(|t| t.name().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use gag::BufferRedirect;
    use insta::assert_snapshot;
    use ironclaw::{
        config::Config,
        sandbox::DockerStatus,
        tunnel::{NativeTunnel, Tunnel},
    };

    use super::{BootScreenContext, print_startup_info};
    use crate::startup::url_sanitize::sanitize_display_url;

    struct TestTunnel {
        public_url: Option<String>,
    }

    impl NativeTunnel for TestTunnel {
        fn name(&self) -> &str {
            "ngrok"
        }

        fn start<'a>(
            &'a self,
            _local_host: &'a str,
            _local_port: u16,
        ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send + 'a {
            let public_url = self.public_url.clone();
            async move {
                public_url.ok_or_else(|| anyhow::anyhow!("test tunnel should have a public URL"))
            }
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn health_check(&self) -> bool {
            true
        }

        fn public_url(&self) -> Option<String> {
            self.public_url.clone()
        }
    }

    #[tokio::test]
    async fn print_startup_info_matches_snapshot() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let mut config = Config::for_testing(
            tempdir.path().join("test.db"),
            tempdir.path().join("skills"),
            tempdir.path().join("installed-skills"),
        )
        .await
        .expect("test config should be built");
        config.agent.name = "startup-test-agent".to_string();
        config.llm.backend = "openai".to_string();
        config.channels.cli.enabled = true;
        config.embeddings.enabled = true;
        config.embeddings.provider = "openai".to_string();
        config.heartbeat.enabled = true;
        config.heartbeat.interval_secs = 1_800;
        config.sandbox.enabled = true;
        config.claude_code.enabled = true;
        config.routines.enabled = true;
        config.skills.enabled = true;
        config.tunnel.public_url = Some("https://fallback.example.test".to_string());

        let cli = ironclaw::cli::Cli {
            command: None,
            cli_only: false,
            no_db: false,
            message: None,
            config: None,
            no_onboard: false,
        };

        let active_tunnel: Option<Box<dyn Tunnel>> = Some(Box::new(TestTunnel {
            public_url: Some("https://runtime.ngrok.app".to_string()),
        }));
        let data = BootScreenContext {
            llm_model: "gpt-4.1".to_string(),
            cheap_model: Some("gpt-4.1-mini".to_string()),
            tool_count: 42,
            gateway_url: Some("http://127.0.0.1:4040/?token=startup-token".to_string()),
            docker_status: DockerStatus::NotRunning,
            channel_names: vec![
                "repl".to_string(),
                "gateway".to_string(),
                "signal".to_string(),
            ],
            active_tunnel: &active_tunnel,
        };

        let capture = BufferRedirect::stdout().expect("stdout capture should be available");
        print_startup_info(&config, &cli, &data);
        let mut output = String::new();
        capture
            .into_inner()
            .read_to_string(&mut output)
            .expect("captured stdout should be readable");
        assert_snapshot!("startup_info_boot_screen", output);
    }

    #[tokio::test]
    async fn print_startup_info_skips_when_message_flag_present() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let mut config = Config::for_testing(
            tempdir.path().join("test.db"),
            tempdir.path().join("skills"),
            tempdir.path().join("installed-skills"),
        )
        .await
        .expect("test config should be built");
        config.channels.cli.enabled = true;

        let cli = ironclaw::cli::Cli {
            command: None,
            cli_only: false,
            no_db: false,
            message: Some("hello".to_string()),
            config: None,
            no_onboard: false,
        };

        let active_tunnel: Option<Box<dyn Tunnel>> = None;
        let data = BootScreenContext {
            llm_model: "gpt-4.1".to_string(),
            cheap_model: None,
            tool_count: 1,
            gateway_url: Some("http://127.0.0.1:4040/".to_string()),
            docker_status: DockerStatus::NotRunning,
            channel_names: vec!["repl".to_string()],
            active_tunnel: &active_tunnel,
        };

        let capture = BufferRedirect::stdout().expect("stdout capture should be available");
        print_startup_info(&config, &cli, &data);
        let mut output = String::new();
        capture
            .into_inner()
            .read_to_string(&mut output)
            .expect("captured stdout should be readable");
        assert!(output.is_empty());
    }

    #[test]
    fn sanitize_display_url_redacts_tokens() {
        let sanitized = sanitize_display_url("http://127.0.0.1:4040/?token=startup-token&mode=1");

        assert_eq!(
            sanitized,
            "http://127.0.0.1:4040/?token=%5BREDACTED%5D&mode=1"
        );
    }

    #[test]
    fn sanitize_display_url_handles_relative_urls() {
        let sanitized = sanitize_display_url("/gateway?token=secret&mode=1");

        assert_eq!(sanitized, "/gateway?token=%5BREDACTED%5D&mode=1");
    }

    #[test]
    fn sanitize_display_url_redacts_authority_credentials() {
        let sanitized =
            sanitize_display_url("https://user:secret@example.test/path?token=startup-token");

        assert_eq!(sanitized, "https://example.test/path?token=%5BREDACTED%5D");
    }

    #[test]
    fn sanitize_display_url_preserves_empty_query_marker() {
        let sanitized = sanitize_display_url("https://example.test/path?");

        assert_eq!(sanitized, "https://example.test/path?");
    }
}
