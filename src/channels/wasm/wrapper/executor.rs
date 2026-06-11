//! WASM store creation, instantiation, and callback execution helpers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use wasmtime::Store;
use wasmtime::component::{HasSelf, Linker};

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::host::{ChannelHostState, ChannelWorkspaceStore, EmittedMessage};
use crate::channels::wasm::runtime::{PreparedChannelModule, WasmChannelRuntime};
use crate::pairing::PairingStore;

use super::types::{ChannelName, SecretValue};
use super::{
    ChannelStoreData, ResolvedHostCredential, SandboxedChannel, WasmChannel, near, wit_channel,
};

impl WasmChannel {
    /// Add channel host functions to the linker using generated bindings.
    ///
    /// Uses the wasmtime::component::bindgen! generated `add_to_linker` function
    /// to properly register all host functions with correct component model signatures.
    pub(super) fn add_host_functions(
        linker: &mut Linker<ChannelStoreData>,
    ) -> Result<(), WasmChannelError> {
        // Add WASI support (required by the component adapter)
        wasmtime_wasi::p2::add_to_linker_sync(linker).map_err(|e| {
            WasmChannelError::Config(format!("Failed to add WASI functions: {}", e))
        })?;

        // Use the generated add_to_linker function from bindgen for our custom interface
        near::agent::channel_host::add_to_linker::<_, HasSelf<_>>(linker, |state| state).map_err(
            |e| WasmChannelError::Config(format!("Failed to add host functions: {}", e)),
        )?;

        Ok(())
    }

    /// Create a fresh store configured for WASM execution.
    pub(super) fn create_store(
        runtime: &WasmChannelRuntime,
        prepared: &PreparedChannelModule,
        capabilities: &ChannelCapabilities,
        credentials: HashMap<String, SecretValue>,
        host_credentials: Vec<ResolvedHostCredential>,
        pairing_store: Arc<PairingStore>,
    ) -> Result<Store<ChannelStoreData>, WasmChannelError> {
        let engine = runtime.engine();
        let limits = &prepared.limits;
        let channel_name = ChannelName::new(&prepared.name)
            .ok_or_else(|| WasmChannelError::InvalidName(prepared.name.clone()))?;

        // Create fresh store with channel state (NEAR pattern: fresh instance per call)
        let store_data = ChannelStoreData::new(
            limits.memory_bytes,
            &channel_name,
            capabilities.clone(),
            credentials,
            host_credentials,
            pairing_store,
        );
        let mut store = Store::new(engine, store_data);

        // Configure fuel if enabled
        if runtime.config().fuel_config.enabled {
            store
                .set_fuel(limits.fuel)
                .map_err(|e| WasmChannelError::Config(format!("Failed to set fuel: {}", e)))?;
        }

        // Configure epoch deadline for timeout backup
        store.epoch_deadline_trap();
        store.set_epoch_deadline(1);

        // Set up resource limiter
        store.limiter(|data| &mut data.limiter);

        Ok(store)
    }

    /// Instantiate the WASM component using generated bindings.
    pub(super) fn instantiate_component(
        runtime: &WasmChannelRuntime,
        prepared: &PreparedChannelModule,
        store: &mut Store<ChannelStoreData>,
    ) -> Result<SandboxedChannel, WasmChannelError> {
        let engine = runtime.engine();

        // Use the pre-compiled component (no recompilation needed)
        let component = prepared
            .component()
            .ok_or_else(|| {
                WasmChannelError::Compilation("No compiled component available".to_string())
            })?
            .clone();

        // Create linker and add host functions
        let mut linker = Linker::new(engine);
        Self::add_host_functions(&mut linker)?;

        // Instantiate using the generated bindings
        let instance = SandboxedChannel::instantiate(store, &component, &linker).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("near:agent") || msg.contains("import") {
                WasmChannelError::Instantiation(format!(
                    "{msg}. This may indicate a WIT version mismatch — \
                         the channel was compiled against a different WIT than the host supports \
                         (host WIT: {}). Rebuild the channel against the current WIT.",
                    crate::tools::wasm::WIT_CHANNEL_VERSION
                ))
            } else {
                WasmChannelError::Instantiation(msg)
            }
        })?;

        Ok(instance)
    }

    /// Map WASM execution errors to our error types.
    pub(super) fn map_wasm_error(
        e: wasmtime::Error,
        name: &str,
        fuel_limit: u64,
    ) -> WasmChannelError {
        let error_str = e.to_string();
        if error_str.contains("out of fuel") {
            WasmChannelError::FuelExhausted {
                name: name.to_string(),
                limit: fuel_limit,
            }
        } else if error_str.contains("unreachable") {
            WasmChannelError::Trapped {
                name: name.to_string(),
                reason: "unreachable code executed".to_string(),
            }
        } else {
            WasmChannelError::Trapped {
                name: name.to_string(),
                reason: error_str,
            }
        }
    }

    /// Extract host state after callback execution.
    pub(super) fn extract_host_state(
        store: &mut Store<ChannelStoreData>,
        channel_name: &str,
        capabilities: &ChannelCapabilities,
    ) -> ChannelHostState {
        std::mem::replace(
            &mut store.data_mut().host_state,
            ChannelHostState::new(channel_name, capabilities.clone()),
        )
    }

    /// Execute a single on_status callback with a fresh WASM instance.
    ///
    /// Static method for use by the background typing repeat task (which
    /// doesn't have access to `&self`).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_status(
        channel_name: &str,
        runtime: &Arc<WasmChannelRuntime>,
        prepared: &Arc<PreparedChannelModule>,
        capabilities: &ChannelCapabilities,
        credentials: &RwLock<HashMap<String, SecretValue>>,
        host_credentials: Vec<ResolvedHostCredential>,
        pairing_store: Arc<PairingStore>,
        timeout: Duration,
        wit_update: wit_channel::StatusUpdate,
    ) -> Result<(), WasmChannelError> {
        if prepared.component().is_none() {
            return Ok(());
        }

        let runtime = Arc::clone(runtime);
        let prepared = Arc::clone(prepared);
        let capabilities = capabilities.clone();
        let credentials_snapshot = credentials.read().await.clone();
        let channel_name_owned = channel_name.to_string();

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials_snapshot,
                    host_credentials,
                    pairing_store,
                )?;
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                let channel_iface = instance.near_agent_channel();
                channel_iface
                    .call_on_status(&mut store, &wit_update)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                Ok(())
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name_owned.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name.to_string(),
                callback: "on_status".to_string(),
            }),
        }
    }

    /// Execute a single poll callback with a fresh WASM instance.
    ///
    /// Returns any emitted messages from the callback. Pending workspace writes
    /// are committed to the shared `ChannelWorkspaceStore` so state persists
    /// across poll ticks (e.g., Telegram polling offset).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_poll(
        channel_name: &str,
        runtime: &Arc<WasmChannelRuntime>,
        prepared: &Arc<PreparedChannelModule>,
        capabilities: &ChannelCapabilities,
        credentials: &RwLock<HashMap<String, SecretValue>>,
        host_credentials: Vec<ResolvedHostCredential>,
        pairing_store: Arc<PairingStore>,
        timeout: Duration,
        workspace_store: &Arc<ChannelWorkspaceStore>,
    ) -> Result<Vec<EmittedMessage>, WasmChannelError> {
        // Skip if no WASM bytes (testing mode)
        if prepared.component().is_none() {
            tracing::debug!(
                channel = %channel_name,
                "WASM channel on_poll called (no WASM module)"
            );
            return Ok(Vec::new());
        }

        let runtime = Arc::clone(runtime);
        let prepared = Arc::clone(prepared);
        let capabilities = Self::inject_workspace_reader(capabilities, workspace_store);
        let credentials_snapshot = credentials.read().await.clone();
        let channel_name_owned = channel_name.to_string();
        let workspace_store = Arc::clone(workspace_store);

        // Execute in blocking task with timeout
        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials_snapshot,
                    host_credentials,
                    pairing_store,
                )?;
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Call on_poll using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                channel_iface
                    .call_on_poll(&mut store)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                let mut host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);

                // Commit pending workspace writes to the persistent store
                let pending_writes = host_state.take_pending_writes();
                workspace_store.commit_writes(&pending_writes);

                Ok(host_state)
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name_owned.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        match result {
            Ok(Ok(mut host_state)) => {
                let emitted = host_state.take_emitted_messages();
                tracing::debug!(
                    channel = %channel_name,
                    emitted_count = emitted.len(),
                    "WASM channel on_poll completed"
                );
                Ok(emitted)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name.to_string(),
                callback: "on_poll".to_string(),
            }),
        }
    }
}
