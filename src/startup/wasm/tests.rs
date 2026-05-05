//! Tests for startup WASM channel bootstrap and runtime wiring.

use std::collections::HashSet;
use std::sync::Arc;

use ironclaw::{
    app::{AppBuilder, AppBuilderFlags, AppComponents},
    channels::web::log_layer::LogBroadcaster,
    channels::{
        ChannelManager,
        wasm::{WasmChannelRouter, WasmChannelRuntime, WasmChannelRuntimeConfig},
        web::types::SseEvent,
    },
    config::Config,
    extensions::{ActivateResult, ExtensionError, ExtensionKind},
    llm::create_session_manager,
    pairing::PairingStore,
};
use tokio::sync::Mutex;

use crate::startup::channels::ChannelRegistrar;

use super::{
    WasmChannelRuntimeState, WasmRuntimeWiringPort, WasmWiringContext, init_wasm_channels,
    wire_wasm_channel_runtime,
};

async fn build_test_components(config: Config, no_db: bool) -> anyhow::Result<AppComponents> {
    let tempdir = tempfile::tempdir()?;
    let session = create_session_manager(config.llm.session.clone()).await;
    let log_broadcaster = Arc::new(LogBroadcaster::new());
    let (components, _side_effects) = AppBuilder::new(
        config,
        AppBuilderFlags {
            no_db,
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

#[derive(Default)]
struct FakeRuntimeManager {
    active_channels: Mutex<Vec<String>>,
    runtime_wire_count: Mutex<usize>,
    persisted_channels: Mutex<Vec<String>>,
    relay_channels: Mutex<HashSet<String>>,
    activation_failures: Mutex<HashSet<String>>,
    wasm_activation_calls: Mutex<Vec<String>>,
    relay_manager_wire_count: Mutex<usize>,
    restore_relay_calls: Mutex<usize>,
    sse_sender: Mutex<Option<tokio::sync::broadcast::Sender<SseEvent>>>,
}

impl FakeRuntimeManager {
    async fn with_persisted_channels(names: &[&str]) -> Arc<Self> {
        let manager = Arc::new(Self::default());
        *manager.persisted_channels.lock().await =
            names.iter().map(|name| (*name).to_string()).collect();
        manager
    }
}

impl WasmRuntimeWiringPort for FakeRuntimeManager {
    async fn set_active_channels(&self, names: Vec<String>) {
        *self.active_channels.lock().await = names;
    }

    async fn set_channel_runtime(
        &self,
        _channel_manager: Arc<ChannelManager>,
        _wasm_channel_runtime: Arc<WasmChannelRuntime>,
        _pairing_store: Arc<PairingStore>,
        _wasm_channel_router: Arc<WasmChannelRouter>,
        _wasm_channel_owner_ids: std::collections::HashMap<String, i64>,
    ) {
        *self.runtime_wire_count.lock().await += 1;
    }

    async fn load_persisted_active_channels(&self) -> Vec<String> {
        self.persisted_channels.lock().await.clone()
    }

    async fn is_relay_channel(&self, name: &str) -> bool {
        self.relay_channels.lock().await.contains(name)
    }

    async fn activate(&self, name: &str) -> Result<ActivateResult, ExtensionError> {
        self.wasm_activation_calls
            .lock()
            .await
            .push(name.to_string());
        if self.activation_failures.lock().await.contains(name) {
            return Err(ExtensionError::ActivationFailed(format!(
                "injected activation failure for {name}"
            )));
        }

        Ok(ActivateResult {
            name: name.to_string(),
            kind: ExtensionKind::WasmChannel,
            tools_loaded: Vec::new(),
            message: format!("activated {name}"),
        })
    }

    async fn set_relay_channel_manager(&self, _channel_manager: Arc<ChannelManager>) {
        *self.relay_manager_wire_count.lock().await += 1;
    }

    async fn restore_relay_channels(&self) {
        *self.restore_relay_calls.lock().await += 1;
    }

    async fn set_sse_sender(&self, sender: tokio::sync::broadcast::Sender<SseEvent>) {
        *self.sse_sender.lock().await = Some(sender);
    }
}

fn test_runtime_state() -> anyhow::Result<WasmChannelRuntimeState> {
    let tempdir = tempfile::tempdir()?;
    Ok((
        Arc::new(WasmChannelRuntime::new(
            WasmChannelRuntimeConfig::for_testing(),
        )?),
        Arc::new(PairingStore::with_base_dir(
            tempdir.path().join("pairing-store"),
        )),
        Arc::new(WasmChannelRouter::new()),
    ))
}

fn test_channels() -> Arc<ChannelManager> {
    Arc::new(ChannelManager::new())
}

fn empty_wasm_channel_owner_ids() -> std::collections::HashMap<String, i64> {
    std::collections::HashMap::new()
}

/// Runs [`init_wasm_channels`] with a throwaway [`ChannelRegistrar`] and
/// returns only the [`super::WasmChannelsInit`] result.
async fn run_init_wasm_channels(
    config: &Config,
    components: &AppComponents,
) -> super::WasmChannelsInit {
    let mut channel_names: Vec<String> = Vec::new();
    let mut webhook_routes: Vec<axum::Router> = Vec::new();
    let channels = ironclaw::channels::ChannelManager::new();
    let mut reg = ChannelRegistrar {
        channels: &channels,
        channel_names: &mut channel_names,
        webhook_routes: &mut webhook_routes,
    };
    init_wasm_channels(config, components, &mut reg).await
}

struct WasmWiringFixture {
    extension_manager: Option<Arc<FakeRuntimeManager>>,
    channels: Arc<ChannelManager>,
    sse_sender: Option<tokio::sync::broadcast::Sender<SseEvent>>,
    wasm_channel_owner_ids: std::collections::HashMap<String, i64>,
}

impl WasmWiringFixture {
    fn new(extension_manager: Option<Arc<FakeRuntimeManager>>) -> Self {
        Self {
            extension_manager,
            channels: test_channels(),
            sse_sender: None,
            wasm_channel_owner_ids: empty_wasm_channel_owner_ids(),
        }
    }

    async fn wire(
        &self,
        runtime_state: &mut Option<WasmChannelRuntimeState>,
        loaded_wasm_channel_names: &[String],
    ) {
        let wiring = WasmWiringContext {
            extension_manager: &self.extension_manager,
            channels: &self.channels,
            sse_sender: &self.sse_sender,
            wasm_channel_owner_ids: &self.wasm_channel_owner_ids,
        };
        wire_wasm_channel_runtime(&wiring, runtime_state, loaded_wasm_channel_names).await;
    }

    fn manager(&self) -> &Arc<FakeRuntimeManager> {
        self.extension_manager.as_ref().expect("manager present")
    }
}

async fn new_test_config() -> anyhow::Result<(tempfile::TempDir, Config)> {
    let tempdir = tempfile::tempdir()?;
    let config = Config::for_testing(
        tempdir.path().join("test.db"),
        tempdir.path().join("skills"),
        tempdir.path().join("installed-skills"),
    )
    .await?;
    Ok((tempdir, config))
}

async fn build_components_for(config: &Config) -> anyhow::Result<AppComponents> {
    build_test_components(config.clone(), true).await
}

async fn wire_with(
    manager: Arc<FakeRuntimeManager>,
    loaded: &[&str],
) -> anyhow::Result<(WasmWiringFixture, Option<WasmChannelRuntimeState>)> {
    let fixture = WasmWiringFixture::new(Some(manager));
    let mut runtime_state = Some(test_runtime_state()?);
    let loaded_wasm_channel_names = loaded
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    fixture
        .wire(&mut runtime_state, &loaded_wasm_channel_names)
        .await;
    Ok((fixture, runtime_state))
}

#[tokio::test]
async fn init_wasm_channels_skips_when_disabled() -> anyhow::Result<()> {
    let (_tempdir, mut config) = new_test_config().await?;
    config.channels.wasm_channels_enabled = false;

    let components = build_components_for(&config).await?;
    let result = run_init_wasm_channels(&config, &components).await;

    assert!(result.loaded_wasm_channel_names.is_empty());
    assert!(result.runtime_state.is_none());
    Ok(())
}

#[tokio::test]
async fn init_wasm_channels_warns_when_directory_missing() -> anyhow::Result<()> {
    let (tempdir, mut config) = new_test_config().await?;
    config.channels.wasm_channels_enabled = true;
    config.channels.wasm_channels_dir = tempdir.path().join("nonexistent");

    let components = build_components_for(&config).await?;
    let result = run_init_wasm_channels(&config, &components).await;

    assert!(result.loaded_wasm_channel_names.is_empty());
    assert!(result.runtime_state.is_none());
    Ok(())
}

#[tokio::test]
async fn wire_wasm_channel_runtime_leaves_state_untouched_without_manager() -> anyhow::Result<()> {
    let fixture = WasmWiringFixture::new(None);
    let mut runtime_state = Some(test_runtime_state()?);
    let loaded = Vec::new();
    fixture.wire(&mut runtime_state, &loaded).await;

    assert!(runtime_state.is_some());
    Ok(())
}

#[tokio::test]
async fn wire_wasm_channel_runtime_skips_relay_channels() -> anyhow::Result<()> {
    let manager = FakeRuntimeManager::with_persisted_channels(&["relay"]).await;
    manager
        .relay_channels
        .lock()
        .await
        .insert("relay".to_string());
    let (fixture, runtime_state) = wire_with(manager, &[]).await?;

    let manager = fixture.manager();
    assert!(runtime_state.is_none());
    assert!(manager.wasm_activation_calls.lock().await.is_empty());
    assert_eq!(*manager.runtime_wire_count.lock().await, 1);
    Ok(())
}

#[tokio::test]
async fn wire_wasm_channel_runtime_skips_channels_that_started_active() -> anyhow::Result<()> {
    let fixture = WasmWiringFixture::new(Some(
        FakeRuntimeManager::with_persisted_channels(&["already-active"]).await,
    ));
    let mut runtime_state = Some(test_runtime_state()?);
    let loaded = vec!["already-active".to_string()];
    fixture.wire(&mut runtime_state, &loaded).await;

    let manager = fixture.manager();
    assert!(runtime_state.is_none());
    assert!(manager.wasm_activation_calls.lock().await.is_empty());
    assert_eq!(
        manager.active_channels.lock().await.as_slice(),
        ["already-active"]
    );
    Ok(())
}

#[tokio::test]
async fn wire_wasm_channel_runtime_logs_activation_failures() -> anyhow::Result<()> {
    let manager = FakeRuntimeManager::with_persisted_channels(&["new-channel"]).await;
    manager
        .activation_failures
        .lock()
        .await
        .insert("new-channel".to_string());
    let (fixture, _runtime_state) = wire_with(manager, &[]).await?;

    let manager = fixture.manager();
    assert_eq!(
        manager.wasm_activation_calls.lock().await.as_slice(),
        ["new-channel"]
    );
    Ok(())
}

#[tokio::test]
async fn wire_wasm_channel_runtime_registers_sse_sender() -> anyhow::Result<()> {
    let mut fixture =
        WasmWiringFixture::new(Some(FakeRuntimeManager::with_persisted_channels(&[]).await));
    let (sender, _) = tokio::sync::broadcast::channel::<SseEvent>(4);
    fixture.sse_sender = Some(sender);
    let mut runtime_state = None;
    let loaded = Vec::new();
    fixture.wire(&mut runtime_state, &loaded).await;

    assert!(fixture.manager().sse_sender.lock().await.is_some());
    Ok(())
}
