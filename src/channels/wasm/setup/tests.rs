//! Behavioural tests for loading WASM channels from an environment snapshot.

use super::*;
use crate::channels::wasm::ChannelCapabilities;
use crate::channels::wasm::runtime::PreparedChannelModule;
use crate::channels::wasm::wrapper::WasmChannel;
use crate::secrets::CredentialMapping;
use crate::tools::wasm::{Capabilities, HttpCapability};

/// Credential placeholder declared by [`snapshot_probe_channel`].
const SNAPSHOT_PROBE_TOKEN: &str = "SNAPSHOTPROBE_TOKEN";

/// Build a channel whose capabilities declare one HTTP credential.
///
/// The module has no compiled component, which is enough for setup: credential
/// injection and router registration never instantiate the WASM module.
fn snapshot_probe_channel() -> WasmChannel {
    let runtime = Arc::new(
        WasmChannelRuntime::new(WasmChannelRuntimeConfig::for_testing())
            .expect("create a test WASM runtime"),
    );
    let prepared = Arc::new(PreparedChannelModule::for_testing(
        "snapshotprobe",
        "Snapshot probe channel",
    ));
    let capabilities = ChannelCapabilities::for_channel("snapshotprobe")
        .with_path("/webhook/snapshotprobe")
        .with_tool_capabilities(Capabilities::none().with_http(
            HttpCapability::new(Vec::new()).with_credential(
                "token",
                CredentialMapping::bearer(SNAPSHOT_PROBE_TOKEN, "api.example.com"),
            ),
        ));

    WasmChannel::new(
        runtime,
        prepared,
        capabilities,
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    )
}

/// Register the probe channel against a request built from `env_context`.
async fn register_snapshot_probe(
    env_context: &EnvContext,
) -> anyhow::Result<(String, Arc<WasmChannel>, Arc<WasmChannelRouter>)> {
    let config = crate::testing::test_utils::test_config_from_context(env_context).await?;
    let secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>> = None;
    let request = WasmChannelSetupRequest {
        config: &config,
        secrets_store: &secrets_store,
        extension_manager: None,
        database: None,
        env_context,
    };
    let router = Arc::new(WasmChannelRouter::new());

    let (name, channel) = register_channel(
        LoadedChannel {
            channel: snapshot_probe_channel(),
            capabilities_file: None,
        },
        &request,
        &router,
    )
    .await;

    Ok((name, channel, router))
}

/// A credential declared by the channel must be injected from the supplied
/// environment snapshot, and the channel must be registered with the router.
#[tokio::test]
async fn register_channel_injects_credentials_from_the_supplied_snapshot() -> anyhow::Result<()> {
    let env_context = EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env(SNAPSHOT_PROBE_TOKEN, "snapshot-token");

    let (name, channel, router) = register_snapshot_probe(&env_context).await?;

    assert_eq!(name, "snapshotprobe");
    assert_eq!(
        channel
            .get_credentials()
            .await
            .get(SNAPSHOT_PROBE_TOKEN)
            .map(String::as_str),
        Some("snapshot-token"),
        "the credential must come from the supplied environment snapshot"
    );
    assert_eq!(
        router.list_channels().await,
        vec!["snapshotprobe".to_string()],
        "the loaded channel must be registered with the router"
    );
    Ok(())
}

/// Setup must not fall back to the ambient process environment.
///
/// The snapshot is empty, so a credential injected here could only have come
/// from the real process environment. Reading ambient state is permitted;
/// mutating it is not. If the host happens to define the probe variable, the
/// test cannot distinguish the two sources, so it skips rather than assert
/// something it cannot prove.
#[tokio::test]
async fn register_channel_does_not_read_the_ambient_environment() -> anyhow::Result<()> {
    if EnvContext::capture_ambient()
        .get_owned(SNAPSHOT_PROBE_TOKEN)
        .is_some()
    {
        return Ok(());
    }

    let env_context = EnvContext::default().with_env("DATABASE_BACKEND", "libsql");
    let (_name, channel, _router) = register_snapshot_probe(&env_context).await?;

    assert!(
        channel.get_credentials().await.is_empty(),
        "an empty snapshot must not yield credentials from the ambient environment"
    );
    Ok(())
}

/// Setup with no channel modules still produces webhook routes so that
/// channels hot-added later can receive webhooks without a restart.
#[tokio::test]
async fn setup_from_empty_directory_serves_webhooks_without_channels() -> anyhow::Result<()> {
    let channels_dir = tempfile::tempdir()?;
    // The libSQL backend supplies a placeholder database URL, so this test
    // does not need a real PostgreSQL connection to build configuration.
    let env_context =
        crate::testing::test_utils::test_env_context_with(&[("DATABASE_BACKEND", "libsql")]);
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
