//! Tests for boot-screen rendering, snapshots, and `BootInfo` derivation.

use insta::assert_snapshot;
use rstest::rstest;

use super::*;
use crate::cli::Cli;
use crate::config::Config;
use crate::tunnel::{NativeTunnel, Tunnel};

fn assert_boot_snapshot(snapshot_name: &str, output: &str) {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../snapshots");
    settings.bind(|| assert_snapshot!(snapshot_name, output));
}

struct TestTunnel {
    name: &'static str,
    public_url: Option<String>,
}

impl NativeTunnel for TestTunnel {
    fn name(&self) -> &str {
        self.name
    }

    fn start<'a>(
        &'a self,
        _local_host: &'a str,
        _local_port: u16,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send + 'a {
        let url = self
            .public_url
            .clone()
            .expect("test tunnel should have a public URL");
        async move { Ok(url) }
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

fn full_boot_info() -> BootInfo {
    BootInfo {
        version: "0.2.0".to_string(),
        agent_name: "ironclaw".to_string(),
        llm_backend: "nearai".to_string(),
        llm_model: "claude-3-5-sonnet-20241022".to_string(),
        cheap_model: Some("gpt-4o-mini".to_string()),
        db_backend: "libsql".to_string(),
        db_connected: true,
        tool_count: 24,
        gateway_url: Some("http://127.0.0.1:3001/?token=abc123".to_string()),
        embeddings_enabled: true,
        embeddings_provider: Some("openai".to_string()),
        heartbeat_enabled: true,
        heartbeat_interval_secs: 1800,
        sandbox_enabled: true,
        docker_status: DockerStatus::Available,
        claude_code_enabled: false,
        routines_enabled: true,
        skills_enabled: true,
        channels: vec![
            "repl".to_string(),
            "gateway".to_string(),
            "telegram".to_string(),
        ],
        tunnel_url: Some("https://abc123.ngrok.io".to_string()),
        tunnel_provider: Some("ngrok".to_string()),
    }
}

/// Provides a BootInfo with all optional feature fields set to their
/// "disabled / none" state. Individual test helpers override only the
/// fields relevant to their scenario.
fn base_disabled_boot_info() -> BootInfo {
    BootInfo {
        version: String::new(),
        agent_name: String::new(),
        llm_backend: String::new(),
        llm_model: String::new(),
        cheap_model: None,
        db_backend: String::new(),
        db_connected: false,
        tool_count: 0,
        gateway_url: None,
        embeddings_enabled: false,
        embeddings_provider: None,
        heartbeat_enabled: false,
        heartbeat_interval_secs: 0,
        sandbox_enabled: false,
        docker_status: DockerStatus::Disabled,
        claude_code_enabled: false,
        routines_enabled: false,
        skills_enabled: false,
        channels: vec![],
        tunnel_url: None,
        tunnel_provider: None,
    }
}

fn minimal_boot_info() -> BootInfo {
    BootInfo {
        version: "0.2.0".to_string(),
        agent_name: "ironclaw".to_string(),
        llm_backend: "nearai".to_string(),
        llm_model: "gpt-4o".to_string(),
        db_backend: "none".to_string(),
        tool_count: 5,
        ..base_disabled_boot_info()
    }
}

fn no_features_boot_info() -> BootInfo {
    BootInfo {
        version: "0.1.0".to_string(),
        agent_name: "test".to_string(),
        llm_backend: "openai".to_string(),
        llm_model: "gpt-4o".to_string(),
        db_backend: "postgres".to_string(),
        db_connected: true,
        tool_count: 10,
        channels: vec!["repl".to_string()],
        ..base_disabled_boot_info()
    }
}

async fn test_config() -> Config {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let mut config = Config::for_testing(
        tempdir.path().join("test.db"),
        tempdir.path().join("skills"),
        tempdir.path().join("installed-skills"),
    )
    .await
    .expect("test config should be built");
    config.tunnel.public_url = Some("https://fallback.example.test".to_string());
    config
}

fn test_cli(no_db: bool) -> Cli {
    Cli {
        command: None,
        cli_only: false,
        no_db,
        message: None,
        config: None,
        no_onboard: false,
    }
}

fn test_data<'a>(active_tunnel: &'a Option<Box<dyn Tunnel>>) -> BootData<'a> {
    BootData {
        llm_model: "gpt-4.1".to_string(),
        cheap_model: Some("gpt-4.1-mini".to_string()),
        tool_count: 42,
        gateway_url: Some("http://127.0.0.1:4040/?token=startup-token".to_string()),
        docker_status: DockerStatus::NotRunning,
        channel_names: vec!["repl".to_string(), "gateway".to_string()],
        active_tunnel,
    }
}

#[rstest]
#[case::full("render_boot_screen_full_snapshot", full_boot_info())]
#[case::minimal("render_boot_screen_minimal_snapshot", minimal_boot_info())]
#[case::no_features("render_boot_screen_no_features_snapshot", no_features_boot_info())]
fn test_render_boot_screen_snapshot(#[case] snapshot_name: &str, #[case] info: BootInfo) {
    let output = render_boot_screen(&info);
    assert_boot_snapshot(snapshot_name, &output);
}

#[test]
fn test_render_boot_screen_docker_not_installed() {
    let mut info = full_boot_info();
    info.docker_status = DockerStatus::NotInstalled;
    let output = render_boot_screen(&info);
    assert_boot_snapshot("render_boot_screen_docker_not_installed", &output);
}

#[test]
fn test_render_boot_screen_docker_not_running() {
    let mut info = full_boot_info();
    info.docker_status = DockerStatus::NotRunning;
    let output = render_boot_screen(&info);
    assert_boot_snapshot("render_boot_screen_docker_not_running", &output);
}

#[tokio::test]
async fn boot_info_from_config_and_data_handles_no_db_and_fallback_tunnel() {
    let config = test_config().await;
    let cli = test_cli(true);
    let active_tunnel: Option<Box<dyn Tunnel>> = None;

    let info = BootInfo::from_config_and_data(&config, &cli, &test_data(&active_tunnel));

    assert_eq!(info.db_backend, "none");
    assert!(!info.db_connected);
    assert_eq!(
        info.tunnel_url.as_deref(),
        Some("https://fallback.example.test")
    );
    assert_eq!(info.tunnel_provider, None);
}

#[tokio::test]
async fn boot_info_from_config_and_data_uses_fallback_url_when_tunnel_has_no_public_url() {
    let config = test_config().await;
    let cli = test_cli(false);
    let active_tunnel: Option<Box<dyn Tunnel>> = Some(Box::new(TestTunnel {
        name: "ngrok",
        public_url: None,
    }));

    let info = BootInfo::from_config_and_data(&config, &cli, &test_data(&active_tunnel));

    assert_eq!(info.db_backend, config.database.backend.to_string());
    assert!(info.db_connected);
    assert_eq!(
        info.tunnel_url.as_deref(),
        Some("https://fallback.example.test")
    );
    assert_eq!(info.tunnel_provider.as_deref(), Some("ngrok"));
}

#[tokio::test]
async fn boot_info_from_config_and_data_prefers_runtime_tunnel_url() {
    let config = test_config().await;
    let cli = test_cli(false);
    let active_tunnel: Option<Box<dyn Tunnel>> = Some(Box::new(TestTunnel {
        name: "ngrok",
        public_url: Some("https://runtime.ngrok.app".to_string()),
    }));

    let info = BootInfo::from_config_and_data(&config, &cli, &test_data(&active_tunnel));

    assert_eq!(info.db_backend, config.database.backend.to_string());
    assert!(info.db_connected);
    assert_eq!(
        info.tunnel_url.as_deref(),
        Some("https://runtime.ngrok.app")
    );
    assert_eq!(info.tunnel_provider.as_deref(), Some("ngrok"));
}
