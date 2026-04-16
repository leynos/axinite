//! Tests for boot-screen rendering, snapshots, and `BootInfo` derivation.

use insta::assert_snapshot;
use mockall::mock;
use rstest::rstest;

use super::*;
use crate::cli::Cli;
use crate::config::Config;
use crate::tunnel::{Tunnel, TunnelFuture};

fn assert_boot_snapshot(snapshot_name: &str, output: &str) {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../snapshots");
    settings.bind(|| assert_snapshot!(snapshot_name, output));
}

mock! {
    TunnelMetadata {}

    impl TunnelMetadata for TunnelMetadata {
        fn name(&self) -> &str;
        fn public_url(&self) -> Option<String>;
    }
}

trait TunnelMetadata: Send + Sync {
    fn name(&self) -> &str;
    fn public_url(&self) -> Option<String>;
}

struct MockTunnelAdapter {
    metadata: MockTunnelMetadata,
}

impl Tunnel for MockTunnelAdapter {
    fn name(&self) -> &str {
        self.metadata.name()
    }

    fn start<'a>(
        &'a self,
        _local_host: &'a str,
        _local_port: u16,
    ) -> TunnelFuture<'a, anyhow::Result<String>> {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "boot screen tests should not call tunnel.start()"
            ))
        })
    }

    fn stop(&self) -> TunnelFuture<'_, anyhow::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn health_check(&self) -> TunnelFuture<'_, bool> {
        Box::pin(async { true })
    }

    fn public_url(&self) -> Option<String> {
        self.metadata.public_url()
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

fn make_mock_tunnel(name: &'static str, public_url: Option<&str>) -> Box<dyn Tunnel> {
    let mut metadata = MockTunnelMetadata::new();
    metadata.expect_name().return_const(name.to_string());
    metadata
        .expect_public_url()
        .return_const(public_url.map(ToString::to_string));
    Box::new(MockTunnelAdapter { metadata })
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

#[rstest]
#[case::no_db_override(true, "none", false)]
#[tokio::test]
async fn boot_info_from_config_and_data_applies_db_override(
    #[case] no_db: bool,
    #[case] expected_backend: &str,
    #[case] expected_connected: bool,
) {
    let config = test_config().await;
    let cli = test_cli(no_db);
    let active_tunnel: Option<Box<dyn Tunnel>> = None;
    let info = BootInfo::from_config_and_data(&config, &cli, &test_data(&active_tunnel));

    assert_eq!(info.db_backend, expected_backend);
    assert_eq!(info.db_connected, expected_connected);
}

#[rstest]
#[case::no_active_tunnel(
    false,
    None,
    Some("https://fallback.example"),
    Some("https://fallback.example"),
    None
)]
#[case::active_tunnel_without_public_url(
    true,
    None,
    Some("https://fallback.example"),
    Some("https://fallback.example"),
    Some("ngrok")
)]
#[case::active_tunnel_with_public_url(
    true,
    Some("https://live.ngrok.io"),
    Some("https://fallback.example"),
    Some("https://live.ngrok.io"),
    Some("ngrok")
)]
#[tokio::test]
async fn boot_info_from_config_and_data_resolves_tunnel_fields(
    #[case] has_active_tunnel: bool,
    #[case] active_public_url: Option<&str>,
    #[case] fallback_public_url: Option<&str>,
    #[case] expected_url: Option<&str>,
    #[case] expected_provider: Option<&str>,
) {
    let mut config = test_config().await;
    config.tunnel.public_url = fallback_public_url.map(ToString::to_string);
    let cli = test_cli(false);
    let active_tunnel = if has_active_tunnel {
        Some(make_mock_tunnel("ngrok", active_public_url))
    } else {
        None
    };
    let data = test_data(&active_tunnel);

    let info = BootInfo::from_config_and_data(&config, &cli, &data);

    assert_eq!(info.tunnel_url.as_deref(), expected_url);
    assert_eq!(info.tunnel_provider.as_deref(), expected_provider);
}
