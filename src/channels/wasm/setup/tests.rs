//! Behavioural tests for loading WASM channels from an environment snapshot.

use super::*;

/// Setup with no channel modules still produces webhook routes so that
/// channels hot-added later can receive webhooks without a restart.
#[tokio::test]
async fn setup_from_empty_directory_serves_webhooks_without_channels() -> anyhow::Result<()> {
    let channels_dir = tempfile::tempdir()?;
    let env_context = crate::testing::test_utils::test_env_context();
    let mut config = crate::testing::test_utils::test_config_from_context(&env_context).await?;
    config.channels.wasm_channels_dir = channels_dir.path().to_path_buf();

    let secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>> = None;
    let setup = setup_wasm_channels_from_context(WasmChannelSetupRequest {
        config: &config,
        secrets_store: &secrets_store,
        extension_manager: None,
        database: None,
        env_context: &env_context,
    })
    .await
    .expect("setup should succeed for an empty channels directory");

    assert!(
        setup.channels.is_empty(),
        "an empty directory must not register any channels"
    );
    assert!(
        setup.channel_names.is_empty(),
        "an empty directory must not report any channel names"
    );
    assert!(
        setup.webhook_routes.is_some(),
        "webhook routes must exist so hot-added channels can receive webhooks"
    );
    Ok(())
}
