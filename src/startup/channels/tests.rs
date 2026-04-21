//! Tests for startup channel wiring.

use std::sync::Arc;

use ironclaw::{
    app::{AppBuilder, AppBuilderFlags, AppComponents},
    channels::{ChannelManager, web::log_layer::LogBroadcaster},
    cli::Cli,
    config::Config,
    llm::create_session_manager,
};

use super::{GatewaySetup, setup_channels};

async fn build_test_components(config: Config) -> anyhow::Result<AppComponents> {
    let tempdir = tempfile::tempdir()?;
    let session = create_session_manager(config.llm.session.clone()).await;
    let log_broadcaster = Arc::new(LogBroadcaster::new());
    let (components, _side_effects) = AppBuilder::new(
        config,
        AppBuilderFlags {
            no_db: true,
            ..Default::default()
        },
        Some(tempdir.path().join("test.toml")),
        session,
        log_broadcaster,
    )
    .build_components()
    .await?;
    Ok(components)
}

fn cli_only() -> Cli {
    Cli {
        command: None,
        cli_only: true,
        no_db: false,
        message: None,
        config: None,
        no_onboard: false,
    }
}

#[tokio::test]
async fn setup_channels_skips_wasm_and_webhooks_in_cli_only_mode() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut config = Config::for_testing(
        tempdir.path().join("test.db"),
        tempdir.path().join("skills"),
        tempdir.path().join("installed-skills"),
    )
    .await?;
    config.channels.wasm_channels_enabled = true;
    config.channels.wasm_channels_dir = tempdir.path().join("channels");
    tokio::fs::create_dir_all(&config.channels.wasm_channels_dir).await?;

    let components = build_test_components(config.clone()).await?;
    let channels = ChannelManager::new();

    let setup = setup_channels(&cli_only(), &config, &components, &channels).await?;

    assert!(setup.loaded_wasm_channel_names.is_empty());
    assert!(setup.wasm_channel_runtime_state.is_none());
    assert!(setup.webhook_server.is_none());
    Ok(())
}

#[test]
fn gateway_setup_defaults_to_none() {
    let setup = GatewaySetup {
        gateway_url: None,
        sse_sender: None,
        routine_engine_slot: None,
    };

    assert!(setup.gateway_url.is_none());
    assert!(setup.sse_sender.is_none());
    assert!(setup.routine_engine_slot.is_none());
}
