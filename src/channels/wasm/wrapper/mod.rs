//! WASM channel wrapper implementing the Channel trait.
//!
//! Wraps a prepared WASM channel module and provides the Channel interface.
//! Each callback (on_start, on_http_request, on_poll, on_respond) creates
//! a fresh WASM instance for isolation.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                    WasmChannel                               │
//! │                                                              │
//! │   ┌─────────────┐   call_on_*   ┌──────────────────────┐    │
//! │   │   Channel   │ ────────────> │   execute_callback   │    │
//! │   │    Trait    │               │   (fresh instance)   │    │
//! │   └─────────────┘               └──────────┬───────────┘    │
//! │                                            │                 │
//! │                                            ▼                 │
//! │   ┌──────────────────────────────────────────────────────┐  │
//! │   │               ChannelStoreData                       │  │
//! │   │  ┌─────────────┐  ┌──────────────────────────────┐   │  │
//! │   │  │   limiter   │  │      ChannelHostState        │   │  │
//! │   │  └─────────────┘  │  - emitted_messages          │   │  │
//! │   │                   │  - pending_writes            │   │  │
//! │   │                   │  - base HostState (logging)  │   │  │
//! │   │                   └──────────────────────────────┘   │  │
//! │   └──────────────────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::host::{ChannelEmitRateLimiter, ChannelWorkspaceStore};
use crate::channels::wasm::router::RegisteredEndpoint;
use crate::channels::wasm::runtime::{PreparedChannelModule, WasmChannelRuntime};
use crate::channels::wasm::schema::ChannelConfig;
use crate::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;
use crate::pairing::PairingStore;
use crate::secrets::SecretsStore;

mod attachments;
mod convert;
mod credentials;
mod dispatch;
mod executor;
mod metadata;
mod polling;
mod shared;
mod status;
mod store;
mod types;

use attachments::read_attachments;
use convert::{
    clone_wit_status_update, convert_channel_config, convert_http_response, status_to_wit,
    wit_channel,
};
use credentials::resolve_channel_host_credentials;
use metadata::do_update_broadcast_metadata;
use store::{ChannelStoreData, ResolvedHostCredential};
use types::SecretValue;

pub use shared::SharedWasmChannel;

#[cfg(test)]
use attachments::mime_from_extension;
pub use convert::HttpResponse;

// Generate component model bindings from the WIT file
wasmtime::component::bindgen!({
    path: "wit/channel.wit",
    world: "sandboxed-channel",
    with: {
        // Use our own store data type
    },
});

/// A WASM-based channel implementing the Channel trait.
#[allow(dead_code)]
pub struct WasmChannel {
    /// Channel name.
    name: String,

    /// Runtime for WASM execution.
    runtime: Arc<WasmChannelRuntime>,

    /// Prepared module (compiled WASM).
    prepared: Arc<PreparedChannelModule>,

    /// Channel capabilities.
    capabilities: ChannelCapabilities,

    /// Channel configuration JSON (passed to on_start).
    /// Wrapped in RwLock to allow updating before start.
    config_json: RwLock<String>,

    /// Channel configuration returned by on_start.
    channel_config: RwLock<Option<ChannelConfig>>,

    /// Message sender (for emitting messages to the stream).
    /// Wrapped in Arc for sharing with the polling task.
    message_tx: Arc<RwLock<Option<mpsc::Sender<IncomingMessage>>>>,

    /// Pending responses (for synchronous response handling).
    pending_responses: RwLock<HashMap<Uuid, oneshot::Sender<String>>>,

    /// Rate limiter for message emission.
    /// Wrapped in Arc for sharing with the polling task.
    rate_limiter: Arc<RwLock<ChannelEmitRateLimiter>>,

    /// Shutdown signal sender.
    shutdown_tx: RwLock<Option<oneshot::Sender<()>>>,

    /// Polling shutdown signal sender (keeps polling alive while held).
    poll_shutdown_tx: RwLock<Option<oneshot::Sender<()>>>,

    /// Registered HTTP endpoints.
    endpoints: RwLock<Vec<RegisteredEndpoint>>,

    /// Injected credentials for HTTP requests (e.g., bot tokens).
    /// Keys are placeholder names like "TELEGRAM_BOT_TOKEN".
    /// Wrapped in Arc for sharing with the polling task.
    credentials: Arc<RwLock<HashMap<String, SecretValue>>>,

    /// Background task that repeats typing indicators every 4 seconds.
    /// Telegram's "typing..." indicator expires after ~5s, so we refresh it.
    typing_task: RwLock<Option<tokio::task::JoinHandle<()>>>,

    /// Pairing store for DM pairing (guest access control).
    pairing_store: Arc<PairingStore>,

    /// In-memory workspace store persisting writes across callback invocations.
    /// Ensures WASM channels can maintain state (e.g., polling offsets) between ticks.
    workspace_store: Arc<ChannelWorkspaceStore>,

    /// Last-seen message metadata (contains chat_id for broadcast routing).
    /// Populated from incoming messages so `broadcast()` knows where to send.
    last_broadcast_metadata: Arc<tokio::sync::RwLock<Option<String>>>,

    /// Settings store for persisting broadcast metadata across restarts.
    settings_store: Option<Arc<dyn crate::db::SettingsStore>>,

    /// Secrets store for host-based credential injection.
    /// Used to pre-resolve credentials before each WASM callback.
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

impl WasmChannel {
    /// Create a new WASM channel.
    pub fn new(
        runtime: Arc<WasmChannelRuntime>,
        prepared: Arc<PreparedChannelModule>,
        capabilities: ChannelCapabilities,
        config_json: String,
        pairing_store: Arc<PairingStore>,
        settings_store: Option<Arc<dyn crate::db::SettingsStore>>,
    ) -> Self {
        let name = prepared.name.clone();
        let rate_limiter = ChannelEmitRateLimiter::new(capabilities.emit_rate_limit.clone());

        Self {
            name,
            runtime,
            prepared,
            capabilities,
            config_json: RwLock::new(config_json),
            channel_config: RwLock::new(None),
            message_tx: Arc::new(RwLock::new(None)),
            pending_responses: RwLock::new(HashMap::new()),
            rate_limiter: Arc::new(RwLock::new(rate_limiter)),
            shutdown_tx: RwLock::new(None),
            poll_shutdown_tx: RwLock::new(None),
            endpoints: RwLock::new(Vec::new()),
            credentials: Arc::new(RwLock::new(HashMap::new())),
            typing_task: RwLock::new(None),
            pairing_store,
            workspace_store: Arc::new(ChannelWorkspaceStore::new()),
            last_broadcast_metadata: Arc::new(tokio::sync::RwLock::new(None)),
            settings_store,
            secrets_store: None,
        }
    }

    /// Set the secrets store for host-based credential injection.
    ///
    /// When set, credentials declared in the channel's capabilities are
    /// automatically decrypted and injected into HTTP requests based on
    /// the target host (e.g., Bearer token for api.slack.com).
    pub fn with_secrets_store(mut self, store: Arc<dyn SecretsStore + Send + Sync>) -> Self {
        self.secrets_store = Some(store);
        self
    }

    /// Update the channel config before starting.
    ///
    /// Merges the provided values into the existing config JSON.
    /// Call this before `start()` to inject runtime values like tunnel_url.
    pub async fn update_config(&self, updates: HashMap<String, serde_json::Value>) {
        let mut config_guard = self.config_json.write().await;

        // Parse existing config
        let mut config: HashMap<String, serde_json::Value> =
            serde_json::from_str(&config_guard).unwrap_or_default();

        // Merge updates
        for (key, value) in updates {
            config.insert(key, value);
        }

        // Serialize back
        *config_guard = serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());

        tracing::debug!(
            channel = %self.name,
            config = %*config_guard,
            "Updated channel config"
        );
    }

    /// Set a credential for URL injection.
    pub async fn set_credential(&self, name: &str, value: String) {
        self.credentials
            .write()
            .await
            .insert(name.to_string(), SecretValue::new(value));
    }

    /// Get a snapshot of credentials for use in callbacks.
    pub async fn get_credentials(&self) -> HashMap<String, String> {
        self.credentials
            .read()
            .await
            .iter()
            .map(|(name, value)| (name.clone(), value.as_str().to_string()))
            .collect()
    }

    /// Get the channel name.
    pub fn channel_name(&self) -> &str {
        &self.name
    }

    /// Get the channel capabilities.
    pub fn capabilities(&self) -> &ChannelCapabilities {
        &self.capabilities
    }

    /// Get the registered endpoints.
    pub async fn endpoints(&self) -> Vec<RegisteredEndpoint> {
        self.endpoints.read().await.clone()
    }

    /// Inject the workspace store as the reader into a capabilities clone.
    ///
    /// Ensures `workspace_read` capability is present with the store as its reader,
    /// so WASM callbacks can read previously written workspace state.
    fn inject_workspace_reader(
        capabilities: &ChannelCapabilities,
        store: &Arc<ChannelWorkspaceStore>,
    ) -> ChannelCapabilities {
        let mut caps = capabilities.clone();
        let ws_cap = caps
            .tool_capabilities
            .workspace_read
            .get_or_insert_with(|| crate::tools::wasm::WorkspaceCapability {
                allowed_prefixes: Vec::new(),
                reader: None,
            });
        ws_cap.reader = Some(Arc::clone(store) as Arc<dyn crate::tools::wasm::WorkspaceReader>);
        caps
    }

    /// Execute the on_start callback.
    ///
    /// Returns the channel configuration for HTTP endpoint registration.
    /// Call the WASM module's `on_start` callback.
    ///
    /// Typically called once during `start()`, but can be called again after
    /// credentials are refreshed to re-trigger webhook registration and
    /// other one-time setup that depends on credentials.
    pub async fn call_on_start(&self) -> Result<ChannelConfig, WasmChannelError> {
        // If no WASM bytes, return default config (for testing)
        if self.prepared.component().is_none() {
            tracing::info!(
                channel = %self.name,
                "WASM channel on_start called (no WASM module, returning defaults)"
            );
            return Ok(ChannelConfig {
                display_name: self.prepared.description.clone(),
                http_endpoints: Vec::new(),
                poll: None,
            });
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = Self::inject_workspace_reader(&self.capabilities, &self.workspace_store);
        let config_json = self.config_json.read().await.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();
        let workspace_store = self.workspace_store.clone();

        // Execute in blocking task with timeout
        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Call on_start using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                let wasm_result = channel_iface
                    .call_on_start(&mut store, &config_json)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                // Convert the result
                let config = match wasm_result {
                    Ok(wit_config) => convert_channel_config(wit_config),
                    Err(err_msg) => {
                        return Err(WasmChannelError::CallbackFailed {
                            name: prepared.name.clone(),
                            reason: err_msg,
                        });
                    }
                };

                let mut host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);

                // Commit pending workspace writes to the persistent store
                let pending_writes = host_state.take_pending_writes();
                workspace_store.commit_writes(&pending_writes);

                Ok((config, host_state))
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        match result {
            Ok(Ok((config, mut host_state))) => {
                // Surface WASM guest logs (errors/warnings from webhook setup, etc.)
                for entry in host_state.take_logs() {
                    match entry.level {
                        crate::tools::wasm::LogLevel::Error => {
                            tracing::error!(channel = %self.name, "{}", entry.message);
                        }
                        crate::tools::wasm::LogLevel::Warn => {
                            tracing::warn!(channel = %self.name, "{}", entry.message);
                        }
                        _ => {
                            tracing::debug!(channel = %self.name, "{}", entry.message);
                        }
                    }
                }
                tracing::info!(
                    channel = %self.name,
                    display_name = %config.display_name,
                    endpoints = config.http_endpoints.len(),
                    "WASM channel on_start completed"
                );
                Ok(config)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: self.name.clone(),
                callback: "on_start".to_string(),
            }),
        }
    }

    /// Execute the on_http_request callback.
    ///
    /// Called when an HTTP request arrives at a registered endpoint.
    pub async fn call_on_http_request(
        &self,
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        query: &HashMap<String, String>,
        body: &[u8],
        secret_validated: bool,
    ) -> Result<HttpResponse, WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            method = method,
            path = path,
            body_len = body.len(),
            secret_validated = secret_validated,
            "call_on_http_request invoked (webhook received)"
        );

        // Log the body for debugging (truncated at char boundary)
        if let Ok(body_str) = std::str::from_utf8(body) {
            let truncated = if body_str.chars().count() > 1000 {
                format!("{}...", body_str.chars().take(1000).collect::<String>())
            } else {
                body_str.to_string()
            };
            tracing::debug!(body = %truncated, "Webhook request body");
        }

        // Log credentials state (without values)
        let creds = self.get_credentials().await;
        tracing::info!(
            credential_count = creds.len(),
            credential_names = ?creds.keys().collect::<Vec<_>>(),
            "Credentials available for on_http_request"
        );

        // If no WASM bytes, return 200 OK (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                method = method,
                path = path,
                "WASM channel on_http_request called (no WASM module)"
            );
            return Ok(HttpResponse::ok());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = Self::inject_workspace_reader(&self.capabilities, &self.workspace_store);
        let timeout = self.runtime.config().callback_timeout;
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();
        let workspace_store = self.workspace_store.clone();

        // Prepare request data
        let method = method.to_string();
        let path = path.to_string();
        let headers_json = serde_json::to_string(&headers).unwrap_or_default();
        let query_json = serde_json::to_string(&query).unwrap_or_default();
        let body = body.to_vec();

        let channel_name = self.name.clone();

        // Execute in blocking task with timeout
        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Build the WIT request type
                let wit_request = wit_channel::IncomingHttpRequest {
                    method,
                    path,
                    headers_json,
                    query_json,
                    body,
                    secret_validated,
                };

                // Call on_http_request using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                let wit_response = channel_iface
                    .call_on_http_request(&mut store, &wit_request)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                let response = convert_http_response(wit_response);
                let mut host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);

                // Commit pending workspace writes to the persistent store
                let pending_writes = host_state.take_pending_writes();
                workspace_store.commit_writes(&pending_writes);

                Ok((response, host_state))
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        let channel_name = self.name.clone();
        match result {
            Ok(Ok((response, mut host_state))) => {
                // Process emitted messages
                let emitted = host_state.take_emitted_messages();
                self.process_emitted_messages(emitted).await?;

                tracing::debug!(
                    channel = %channel_name,
                    status = response.status,
                    "WASM channel on_http_request completed"
                );
                Ok(response)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_http_request".to_string(),
            }),
        }
    }

    /// Execute the on_poll callback.
    ///
    /// Called periodically if polling is configured.
    pub async fn call_on_poll(&self) -> Result<(), WasmChannelError> {
        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                "WASM channel on_poll called (no WASM module)"
            );
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = Self::inject_workspace_reader(&self.capabilities, &self.workspace_store);
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();
        let workspace_store = self.workspace_store.clone();

        // Execute in blocking task with timeout
        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
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

                Ok(((), host_state))
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        let channel_name = self.name.clone();
        match result {
            Ok(Ok(((), mut host_state))) => {
                // Process emitted messages
                let emitted = host_state.take_emitted_messages();
                self.process_emitted_messages(emitted).await?;

                tracing::debug!(
                    channel = %channel_name,
                    "WASM channel on_poll completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_poll".to_string(),
            }),
        }
    }

    /// Execute the on_respond callback.
    ///
    /// Called when the agent has a response to send back.
    pub async fn call_on_respond(
        &self,
        message_id: Uuid,
        content: &str,
        thread_id: Option<&str>,
        metadata_json: &str,
        attachments: &[String],
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            message_id = %message_id,
            content_len = content.len(),
            thread_id = ?thread_id,
            attachment_count = attachments.len(),
            "call_on_respond invoked"
        );

        // Log credentials state (without values)
        let creds = self.get_credentials().await;
        tracing::info!(
            credential_count = creds.len(),
            credential_names = ?creds.keys().collect::<Vec<_>>(),
            "Credentials available for on_respond"
        );

        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                message_id = %message_id,
                "WASM channel on_respond called (no WASM module)"
            );
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = self.capabilities.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();

        // Prepare response data
        let message_id_str = message_id.to_string();
        let content = content.to_string();
        let thread_id = thread_id.map(|s| s.to_string());
        let metadata_json = metadata_json.to_string();
        let attachments = attachments.to_vec();

        // Execute in blocking task with timeout
        tracing::info!(channel = %channel_name, "Starting on_respond WASM execution");

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                // Read attachment files from disk before entering WASM
                let wit_attachments = read_attachments(&attachments).map_err(|e| {
                    WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: e,
                    }
                })?;

                tracing::info!("Creating WASM store for on_respond");
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;

                tracing::info!("Instantiating WASM component for on_respond");
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Build the WIT response type
                let wit_response = wit_channel::AgentResponse {
                    message_id: message_id_str,
                    content: content.clone(),
                    thread_id,
                    metadata_json,
                    attachments: wit_attachments,
                };

                // Truncate at char boundary for logging (avoid panic on multi-byte UTF-8)
                let content_preview: String = content.chars().take(50).collect();
                tracing::info!(
                    content_preview = %content_preview,
                    "Calling WASM on_respond"
                );

                // Call on_respond using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                let wasm_result = channel_iface
                    .call_on_respond(&mut store, &wit_response)
                    .map_err(|e| {
                        tracing::error!(error = %e, "WASM on_respond call failed");
                        Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel)
                    })?;

                tracing::info!(wasm_result = ?wasm_result, "WASM on_respond returned");

                // Check for WASM-level errors
                if let Err(ref err_msg) = wasm_result {
                    tracing::error!(error = %err_msg, "WASM on_respond returned error");
                    return Err(WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: err_msg.clone(),
                    });
                }

                let host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);
                tracing::info!("on_respond WASM execution completed successfully");
                Ok(((), host_state))
            })
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "spawn_blocking panicked");
                WasmChannelError::ExecutionPanicked {
                    name: channel_name.clone(),
                    reason: e.to_string(),
                }
            })?
        })
        .await;

        let channel_name = self.name.clone();
        match result {
            Ok(Ok(((), _host_state))) => {
                tracing::debug!(
                    channel = %channel_name,
                    message_id = %message_id,
                    "WASM channel on_respond completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_respond".to_string(),
            }),
        }
    }

    /// Execute the on_broadcast callback.
    ///
    /// Called to send a proactive message to a user without a prior incoming message.
    pub async fn call_on_broadcast(
        &self,
        user_id: &str,
        content: &str,
        thread_id: Option<&str>,
        attachments: &[String],
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            user_id = %user_id,
            content_len = content.len(),
            attachment_count = attachments.len(),
            "call_on_broadcast invoked"
        );

        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                "WASM channel on_broadcast called (no WASM module)"
            );
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = self.capabilities.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();

        let user_id = user_id.to_string();
        let content = content.to_string();
        let thread_id = thread_id.map(|s| s.to_string());
        let attachments = attachments.to_vec();

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                // Read attachment files from disk
                let wit_attachments = read_attachments(&attachments).map_err(|e| {
                    WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: e,
                    }
                })?;

                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;

                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                let wit_response = wit_channel::AgentResponse {
                    message_id: String::new(),
                    content: content.clone(),
                    thread_id,
                    metadata_json: String::new(),
                    attachments: wit_attachments,
                };

                let channel_iface = instance.near_agent_channel();
                let wasm_result = channel_iface
                    .call_on_broadcast(&mut store, &user_id, &wit_response)
                    .map_err(|e| {
                        tracing::error!(error = %e, "WASM on_broadcast call failed");
                        Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel)
                    })?;

                if let Err(ref err_msg) = wasm_result {
                    tracing::error!(error = %err_msg, "WASM on_broadcast returned error");
                    return Err(WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: err_msg.clone(),
                    });
                }

                let host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);
                tracing::info!("on_broadcast WASM execution completed successfully");
                Ok(((), host_state))
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        let channel_name = self.name.clone();
        match result {
            Ok(Ok(((), _host_state))) => {
                tracing::debug!(
                    channel = %channel_name,
                    "WASM channel on_broadcast completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_broadcast".to_string(),
            }),
        }
    }

    /// Execute the on_status callback.
    ///
    /// Called to notify the WASM channel of agent status changes (e.g., typing).
    pub async fn call_on_status(
        &self,
        status: &StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), WasmChannelError> {
        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = self.capabilities.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();

        let wit_update = status_to_wit(status, metadata);

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
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
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        match result {
            Ok(Ok(())) => {
                tracing::debug!(
                    channel = %self.name,
                    "WASM channel on_status completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: self.name.clone(),
                callback: "on_status".to_string(),
            }),
        }
    }
}

impl NativeChannel for WasmChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        // Restore broadcast metadata from settings (survives restarts)
        self.load_broadcast_metadata().await;

        // Create message channel
        let (tx, rx) = mpsc::channel(256);
        *self.message_tx.write().await = Some(tx);

        // Create shutdown channel
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        // Call on_start to get configuration
        let config = self
            .call_on_start()
            .await
            .map_err(|e| ChannelError::StartupFailed {
                name: self.name.clone(),
                reason: e.to_string(),
            })?;

        // Store the config
        *self.channel_config.write().await = Some(config.clone());

        // Register HTTP endpoints
        let mut endpoints = Vec::new();
        for endpoint in &config.http_endpoints {
            // Validate path is allowed
            if !self.capabilities.is_path_allowed(&endpoint.path) {
                tracing::warn!(
                    channel = %self.name,
                    path = %endpoint.path,
                    "HTTP endpoint path not allowed by capabilities"
                );
                continue;
            }

            endpoints.push(RegisteredEndpoint {
                channel_name: self.name.clone(),
                path: endpoint.path.clone(),
                methods: endpoint.methods.clone(),
                require_secret: endpoint.require_secret,
            });
        }
        *self.endpoints.write().await = endpoints;

        // Start polling if configured
        if let Some(poll_config) = &config.poll
            && poll_config.enabled
        {
            let interval = self
                .capabilities
                .validate_poll_interval(poll_config.interval_ms)
                .map_err(|e| ChannelError::StartupFailed {
                    name: self.name.clone(),
                    reason: e,
                })?;

            // Create shutdown channel for polling and store the sender to keep it alive
            let (poll_shutdown_tx, poll_shutdown_rx) = oneshot::channel();
            *self.poll_shutdown_tx.write().await = Some(poll_shutdown_tx);

            self.start_polling(Duration::from_millis(interval as u64), poll_shutdown_rx);
        }

        tracing::info!(
            channel = %self.name,
            display_name = %config.display_name,
            endpoints = config.http_endpoints.len(),
            "WASM channel started"
        );

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        // Stop the typing indicator, we're about to send the actual response
        self.cancel_typing_task().await;

        // Check if there's a pending synchronous response waiter
        if let Some(tx) = self.pending_responses.write().await.remove(&msg.id) {
            let _ = tx.send(response.content.clone());
        }

        // Call WASM on_respond
        // IMPORTANT: Use the ORIGINAL message's metadata, not the response's metadata.
        // The original metadata contains channel-specific routing info (e.g., Telegram chat_id)
        // that the WASM channel needs to send the reply to the correct destination.
        let metadata_json = serde_json::to_string(&msg.metadata).unwrap_or_default();
        // Store for broadcast routing (chat_id etc.)
        self.update_broadcast_metadata(&metadata_json).await;
        self.call_on_respond(
            msg.id,
            &response.content,
            response.thread_id.as_deref(),
            &metadata_json,
            &response.attachments,
        )
        .await
        .map_err(|e| ChannelError::SendFailed {
            name: self.name.clone(),
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn broadcast(
        &self,
        user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.cancel_typing_task().await;
        self.call_on_broadcast(
            user_id,
            &response.content,
            response.thread_id.as_deref(),
            &response.attachments,
        )
        .await
        .map_err(|e| ChannelError::SendFailed {
            name: self.name.clone(),
            reason: e.to_string(),
        })
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        // Delegate to the typing indicator implementation
        self.handle_status_update(status, metadata).await
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        // Check if we have an active message sender
        if self.message_tx.read().await.is_some() {
            Ok(())
        } else {
            Err(ChannelError::HealthCheckFailed {
                name: self.name.clone(),
            })
        }
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        // Cancel typing indicator
        self.cancel_typing_task().await;

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        }

        // Stop polling by dropping the sender (receiver will complete)
        let _ = self.poll_shutdown_tx.write().await.take();

        // Clear the message sender
        *self.message_tx.write().await = None;

        tracing::info!(
            channel = %self.name,
            "WASM channel shut down"
        );

        Ok(())
    }
}

impl std::fmt::Debug for WasmChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmChannel")
            .field("name", &self.name)
            .field("prepared", &self.prepared.name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::channels::NativeChannel;
    use crate::channels::wasm::capabilities::ChannelCapabilities;
    use crate::channels::wasm::runtime::{
        PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
    };
    use crate::channels::wasm::wrapper::{HttpResponse, WasmChannel};
    use crate::pairing::PairingStore;
    use crate::testing::credentials::TEST_TELEGRAM_BOT_TOKEN;
    use crate::tools::wasm::ResourceLimits;

    use super::types::{ChannelName, HostPattern, SecretValue};

    fn create_test_channel() -> WasmChannel {
        let config = WasmChannelRuntimeConfig::for_testing();
        let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

        let prepared = Arc::new(PreparedChannelModule {
            name: "test".to_string(),
            description: "Test channel".to_string(),
            component: None,
            limits: ResourceLimits::default(),
        });

        let capabilities = ChannelCapabilities::for_channel("test").with_path("/webhook/test");

        WasmChannel::new(
            runtime,
            prepared,
            capabilities,
            "{}".to_string(),
            Arc::new(PairingStore::new()),
            None,
        )
    }

    #[test]
    fn test_channel_name() {
        let channel = create_test_channel();
        assert_eq!(channel.name(), "test");
    }

    #[test]
    fn test_http_response_ok() {
        let response = HttpResponse::ok();
        assert_eq!(response.status, 200);
        assert!(response.body.is_empty());
    }

    #[test]
    fn test_http_response_json() {
        let response = HttpResponse::json(serde_json::json!({"key": "value"}));
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_http_response_error() {
        let response = HttpResponse::error(400, "Bad request");
        assert_eq!(response.status, 400);
        assert_eq!(response.body, b"Bad request");
    }

    #[tokio::test]
    async fn test_channel_start_and_shutdown() {
        let channel = create_test_channel();

        // Start should succeed
        let stream = channel.start().await;
        assert!(stream.is_ok());

        // Health check should pass
        assert!(channel.health_check().await.is_ok());

        // Shutdown should succeed
        assert!(channel.shutdown().await.is_ok());

        // Health check should fail after shutdown
        assert!(channel.health_check().await.is_err());
    }

    #[tokio::test]
    async fn test_execute_poll_no_wasm_returns_empty() {
        // When there's no WASM module (None component), execute_poll
        // should return an empty vector of messages
        let config = WasmChannelRuntimeConfig::for_testing();
        let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

        let prepared = Arc::new(PreparedChannelModule {
            name: "poll-test".to_string(),
            description: "Test channel".to_string(),
            component: None, // No WASM module
            limits: ResourceLimits::default(),
        });

        let capabilities = ChannelCapabilities::for_channel("poll-test").with_polling(1000);
        let credentials = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
        let timeout = std::time::Duration::from_secs(5);

        let workspace_store = Arc::new(crate::channels::wasm::host::ChannelWorkspaceStore::new());

        let result = WasmChannel::execute_poll(
            "poll-test",
            &runtime,
            &prepared,
            &capabilities,
            &credentials,
            Vec::new(), // no host credentials in test
            Arc::new(PairingStore::new()),
            timeout,
            &workspace_store,
        )
        .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_dispatch_emitted_messages_sends_to_channel() {
        use crate::channels::wasm::host::EmittedMessage;

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

        let rate_limiter = Arc::new(tokio::sync::RwLock::new(
            crate::channels::wasm::host::ChannelEmitRateLimiter::new(
                crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
            ),
        ));

        let messages = vec![
            EmittedMessage::new("user1", "Hello from polling!"),
            EmittedMessage::new("user2", "Another message"),
        ];

        let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
        let result = WasmChannel::dispatch_emitted_messages(
            "test-channel",
            messages,
            &message_tx,
            &rate_limiter,
            &last_broadcast_metadata,
            None,
        )
        .await;

        assert!(result.is_ok());

        // Verify messages were sent
        let msg1 = rx.try_recv().expect("Should receive first message");
        assert_eq!(msg1.user_id, "user1");
        assert_eq!(msg1.content, "Hello from polling!");

        let msg2 = rx.try_recv().expect("Should receive second message");
        assert_eq!(msg2.user_id, "user2");
        assert_eq!(msg2.content, "Another message");

        // No more messages
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_dispatch_emitted_messages_no_sender_returns_ok() {
        use crate::channels::wasm::host::EmittedMessage;

        // No sender available (channel not started)
        let message_tx = Arc::new(tokio::sync::RwLock::new(None));
        let rate_limiter = Arc::new(tokio::sync::RwLock::new(
            crate::channels::wasm::host::ChannelEmitRateLimiter::new(
                crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
            ),
        ));

        let messages = vec![EmittedMessage::new("user1", "Hello!")];

        // Should return Ok even without a sender (logs warning but doesn't fail)
        let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
        let result = WasmChannel::dispatch_emitted_messages(
            "test-channel",
            messages,
            &message_tx,
            &rate_limiter,
            &last_broadcast_metadata,
            None,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_channel_with_polling_stores_shutdown_sender() {
        // Create a channel with polling capabilities
        let config = WasmChannelRuntimeConfig::for_testing();
        let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

        let prepared = Arc::new(PreparedChannelModule {
            name: "poll-channel".to_string(),
            description: "Polling test channel".to_string(),
            component: None,
            limits: ResourceLimits::default(),
        });

        // Enable polling with a 1 second minimum interval
        let capabilities = ChannelCapabilities::for_channel("poll-channel")
            .with_path("/webhook/poll")
            .with_polling(1000);

        let channel = WasmChannel::new(
            runtime,
            prepared,
            capabilities,
            "{}".to_string(),
            Arc::new(PairingStore::new()),
            None,
        );

        // Start the channel
        let _stream = channel.start().await.expect("Channel should start");

        // Verify poll_shutdown_tx is set (polling was started)
        // Note: For testing channels without WASM, on_start returns no poll config,
        // so polling won't actually be started. This verifies the basic lifecycle.
        assert!(channel.health_check().await.is_ok());

        // Shutdown should clean up properly
        channel.shutdown().await.expect("Shutdown should succeed");
        assert!(channel.health_check().await.is_err());
    }

    #[tokio::test]
    async fn test_call_on_poll_no_wasm_succeeds() {
        // Verify call_on_poll returns Ok when there's no WASM module
        let channel = create_test_channel();

        // Start the channel first to set up message_tx
        let _stream = channel.start().await.expect("Channel should start");

        // call_on_poll should succeed (no-op for no WASM)
        let result = channel.call_on_poll().await;
        assert!(result.is_ok());

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[tokio::test]
    async fn test_typing_task_starts_on_thinking() {
        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // Sending Thinking should succeed (no-op for no WASM)
        let result = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("Processing...".into()),
                &metadata,
            )
            .await;
        assert!(result.is_ok());

        // A typing task should have been spawned
        assert!(channel.typing_task.read().await.is_some());

        // Shutdown should cancel the typing task
        channel.shutdown().await.expect("Shutdown should succeed");
        assert!(channel.typing_task.read().await.is_none());
    }

    #[tokio::test]
    async fn test_typing_task_cancelled_on_done() {
        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // Start typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("Processing...".into()),
                &metadata,
            )
            .await;
        assert!(channel.typing_task.read().await.is_some());

        // Send Done status
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Status("Done".into()),
                &metadata,
            )
            .await;

        // Typing task should be cancelled
        assert!(channel.typing_task.read().await.is_none());

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[tokio::test]
    async fn test_typing_task_persists_on_tool_started() {
        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // Start typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("Processing...".into()),
                &metadata,
            )
            .await;
        assert!(channel.typing_task.read().await.is_some());

        // Intermediate tool status should not cancel typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::ToolStarted {
                    name: "http_request".into(),
                },
                &metadata,
            )
            .await;

        assert!(channel.typing_task.read().await.is_some());

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[tokio::test]
    async fn test_typing_task_cancelled_on_approval_needed() {
        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // Start typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("Processing...".into()),
                &metadata,
            )
            .await;
        assert!(channel.typing_task.read().await.is_some());

        // Approval-needed should stop typing while waiting for user action
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::ApprovalNeeded {
                    request_id: "req-1".into(),
                    tool_name: "http_request".into(),
                    description: "Fetch weather".into(),
                    parameters: serde_json::json!({"url": "https://wttr.in"}),
                },
                &metadata,
            )
            .await;

        assert!(channel.typing_task.read().await.is_none());

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[tokio::test]
    async fn test_typing_task_cancelled_on_awaiting_approval_status() {
        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // Start typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("Processing...".into()),
                &metadata,
            )
            .await;
        assert!(channel.typing_task.read().await.is_some());

        // Legacy terminal status string should also cancel typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Status("Awaiting approval".into()),
                &metadata,
            )
            .await;

        assert!(channel.typing_task.read().await.is_none());

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[tokio::test]
    async fn test_typing_task_replaced_on_new_thinking() {
        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // Start typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("First...".into()),
                &metadata,
            )
            .await;

        // Get handle of first task
        let first_handle = {
            let guard = channel.typing_task.read().await;
            guard.as_ref().map(|h| h.id())
        };
        assert!(first_handle.is_some());

        // Start typing again (should replace the previous task)
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("Second...".into()),
                &metadata,
            )
            .await;

        // Should still have a typing task, but it's a new one
        let second_handle = {
            let guard = channel.typing_task.read().await;
            guard.as_ref().map(|h| h.id())
        };
        assert!(second_handle.is_some());
        // The task IDs should differ (old one was aborted, new one spawned)
        assert_ne!(first_handle, second_handle);

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[tokio::test]
    async fn test_respond_cancels_typing_task() {
        use crate::channels::IncomingMessage;

        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // Start typing
        let _ = channel
            .send_status(
                crate::channels::StatusUpdate::Thinking("Processing...".into()),
                &metadata,
            )
            .await;
        assert!(channel.typing_task.read().await.is_some());

        // Respond should cancel the typing task
        let msg = IncomingMessage::new("test", "user1", "hello").with_metadata(metadata);
        let _ = channel
            .respond(&msg, crate::channels::OutgoingResponse::text("response"))
            .await;

        // Typing task should be gone
        assert!(channel.typing_task.read().await.is_none());

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[tokio::test]
    async fn test_stream_chunk_is_noop() {
        let channel = create_test_channel();
        let _stream = channel.start().await.expect("Channel should start");

        let metadata = serde_json::json!({"chat_id": 123});

        // StreamChunk should not start a typing task
        let result = channel
            .send_status(
                crate::channels::StatusUpdate::StreamChunk("chunk".into()),
                &metadata,
            )
            .await;
        assert!(result.is_ok());
        assert!(channel.typing_task.read().await.is_none());

        channel.shutdown().await.expect("Shutdown should succeed");
    }

    #[test]
    fn test_status_to_wit_thinking() {
        use super::status_to_wit;

        let metadata = serde_json::json!({"chat_id": 42});
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Thinking("Processing...".into()),
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::Thinking
        ));
        assert_eq!(wit.message, "Processing...");
        assert!(wit.metadata_json.contains("42"));
    }

    #[test]
    fn test_status_to_wit_done() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Status("Done".into()),
            &metadata,
        );

        assert!(matches!(wit.status, super::wit_channel::StatusType::Done));
    }

    #[test]
    fn test_status_to_wit_done_case_insensitive() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);

        // lowercase
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Status("done".into()),
            &metadata,
        );
        assert!(matches!(wit.status, super::wit_channel::StatusType::Done));

        // with whitespace
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Status(" Done ".into()),
            &metadata,
        );
        assert!(matches!(wit.status, super::wit_channel::StatusType::Done));
    }

    #[test]
    fn test_status_to_wit_interrupted() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Status("Interrupted".into()),
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::Interrupted
        ));
    }

    #[test]
    fn test_status_to_wit_interrupted_case_insensitive() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);

        // lowercase
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Status("interrupted".into()),
            &metadata,
        );
        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::Interrupted
        ));

        // with whitespace
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Status(" Interrupted ".into()),
            &metadata,
        );
        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::Interrupted
        ));
    }

    #[test]
    fn test_status_to_wit_generic_status() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::Status("Awaiting approval".into()),
            &metadata,
        );

        assert!(matches!(wit.status, super::wit_channel::StatusType::Status));
        assert_eq!(wit.message, "Awaiting approval");
    }

    #[test]
    fn test_status_to_wit_auth_required() {
        use super::status_to_wit;

        let metadata = serde_json::json!({"chat_id": 42});
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::AuthRequired {
                extension_name: "weather".to_string(),
                instructions: Some("Paste your token".to_string()),
                auth_url: Some("https://example.com/auth".to_string()),
                setup_url: None,
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::AuthRequired
        ));
        assert!(wit.message.contains("Authentication required for weather"));
        assert!(wit.message.contains("Paste your token"));
    }

    #[test]
    fn test_status_to_wit_tool_started() {
        use super::status_to_wit;

        let metadata = serde_json::json!({"chat_id": 7});
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::ToolStarted {
                name: "http_request".to_string(),
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::ToolStarted
        ));
        assert_eq!(wit.message, "Tool started: http_request");
    }

    #[test]
    fn test_status_to_wit_tool_completed_success() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::ToolCompleted {
                name: "http_request".to_string(),
                success: true,
                error: None,
                parameters: None,
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::ToolCompleted
        ));
        assert_eq!(wit.message, "Tool completed: http_request (ok)");
    }

    #[test]
    fn test_status_to_wit_tool_completed_failure() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::ToolCompleted {
                name: "http_request".to_string(),
                success: false,
                error: Some("connection refused".to_string()),
                parameters: None,
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::ToolCompleted
        ));
        assert_eq!(wit.message, "Tool completed: http_request (failed)");
    }

    #[test]
    fn test_status_to_wit_tool_result() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::ToolResult {
                name: "http_request".to_string(),
                preview: "{".to_string() + "\"temperature\": 22}",
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::ToolResult
        ));
        assert!(wit.message.starts_with("Tool result: http_request\n"));
    }

    #[test]
    fn test_status_to_wit_tool_result_truncates_preview() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let long_preview = "x".repeat(400);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::ToolResult {
                name: "big_tool".to_string(),
                preview: long_preview,
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::ToolResult
        ));
        assert!(wit.message.ends_with("..."));
    }

    #[test]
    fn test_status_to_wit_job_started() {
        use super::status_to_wit;

        let metadata = serde_json::json!({"chat_id": 1});
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::JobStarted {
                job_id: "job-1".to_string(),
                title: "Daily sync".to_string(),
                browse_url: "https://example.com/jobs/job-1".to_string(),
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::JobStarted
        ));
        assert!(wit.message.contains("Daily sync"));
        assert!(wit.message.contains("https://example.com/jobs/job-1"));
    }

    #[test]
    fn test_status_to_wit_auth_completed_success() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::AuthCompleted {
                extension_name: "weather".to_string(),
                success: true,
                message: "Token saved".to_string(),
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::AuthCompleted
        ));
        assert!(wit.message.contains("Authentication completed"));
        assert!(wit.message.contains("Token saved"));
    }

    #[test]
    fn test_status_to_wit_auth_completed_failure() {
        use super::status_to_wit;

        let metadata = serde_json::json!(null);
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::AuthCompleted {
                extension_name: "weather".to_string(),
                success: false,
                message: "Invalid token".to_string(),
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::AuthCompleted
        ));
        assert!(wit.message.contains("Authentication failed"));
        assert!(wit.message.contains("Invalid token"));
    }

    #[test]
    fn test_status_to_wit_approval_needed() {
        use super::status_to_wit;

        let metadata = serde_json::json!({"chat_id": 42});
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::ApprovalNeeded {
                request_id: "req-123".to_string(),
                tool_name: "http_request".to_string(),
                description: "Fetch weather data".to_string(),
                parameters: serde_json::json!({"url": "https://api.weather.test"}),
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::ApprovalNeeded
        ));
        assert!(wit.message.contains("http_request"));
        assert!(wit.message.contains("/approve"));
    }

    #[test]
    fn test_approval_prompt_roundtrip_submission_aliases() {
        use super::status_to_wit;
        use crate::agent::submission::{Submission, SubmissionParser};

        let metadata = serde_json::json!({"chat_id": 42});
        let wit = status_to_wit(
            &crate::channels::StatusUpdate::ApprovalNeeded {
                request_id: "req-321".to_string(),
                tool_name: "http_request".to_string(),
                description: "Fetch weather data".to_string(),
                parameters: serde_json::json!({"url": "https://api.weather.test"}),
            },
            &metadata,
        );

        assert!(matches!(
            wit.status,
            super::wit_channel::StatusType::ApprovalNeeded
        ));
        assert!(wit.message.contains("/approve"));
        assert!(wit.message.contains("/deny"));
        assert!(wit.message.contains("/always"));

        let approve = SubmissionParser::parse("/approve");
        assert!(matches!(
            approve,
            Submission::ApprovalResponse {
                approved: true,
                always: false
            }
        ));

        let deny = SubmissionParser::parse("/deny");
        assert!(matches!(
            deny,
            Submission::ApprovalResponse {
                approved: false,
                always: false
            }
        ));

        let always = SubmissionParser::parse("/always");
        assert!(matches!(
            always,
            Submission::ApprovalResponse {
                approved: true,
                always: true
            }
        ));
    }

    #[test]
    fn test_clone_wit_status_update() {
        use super::{clone_wit_status_update, wit_channel};

        let original = wit_channel::StatusUpdate {
            status: wit_channel::StatusType::Thinking,
            message: "hello".to_string(),
            metadata_json: "{\"a\":1}".to_string(),
        };

        let cloned = clone_wit_status_update(&original);
        assert!(matches!(cloned.status, wit_channel::StatusType::Thinking));
        assert_eq!(cloned.message, "hello");
        assert_eq!(cloned.metadata_json, "{\"a\":1}");
    }

    #[test]
    fn test_clone_wit_status_update_approval_needed() {
        use super::{clone_wit_status_update, wit_channel};

        let original = wit_channel::StatusUpdate {
            status: wit_channel::StatusType::ApprovalNeeded,
            message: "approval needed".to_string(),
            metadata_json: "{\"chat_id\":42}".to_string(),
        };

        let cloned = clone_wit_status_update(&original);
        assert!(matches!(
            cloned.status,
            wit_channel::StatusType::ApprovalNeeded
        ));
        assert_eq!(cloned.message, "approval needed");
        assert_eq!(cloned.metadata_json, "{\"chat_id\":42}");
    }

    #[test]
    fn test_clone_wit_status_update_auth_completed() {
        use super::{clone_wit_status_update, wit_channel};

        let original = wit_channel::StatusUpdate {
            status: wit_channel::StatusType::AuthCompleted,
            message: "auth complete".to_string(),
            metadata_json: "{}".to_string(),
        };

        let cloned = clone_wit_status_update(&original);
        assert!(matches!(
            cloned.status,
            wit_channel::StatusType::AuthCompleted
        ));
        assert_eq!(cloned.message, "auth complete");
    }

    #[test]
    fn test_clone_wit_status_update_all_variants() {
        use super::{clone_wit_status_update, wit_channel};

        let variants = vec![
            wit_channel::StatusType::Thinking,
            wit_channel::StatusType::Done,
            wit_channel::StatusType::Interrupted,
            wit_channel::StatusType::ToolStarted,
            wit_channel::StatusType::ToolCompleted,
            wit_channel::StatusType::ToolResult,
            wit_channel::StatusType::ApprovalNeeded,
            wit_channel::StatusType::Status,
            wit_channel::StatusType::JobStarted,
            wit_channel::StatusType::AuthRequired,
            wit_channel::StatusType::AuthCompleted,
        ];

        for status in variants {
            let original = wit_channel::StatusUpdate {
                status,
                message: "sample".to_string(),
                metadata_json: "{}".to_string(),
            };
            let cloned = clone_wit_status_update(&original);

            assert_eq!(
                std::mem::discriminant(&cloned.status),
                std::mem::discriminant(&original.status)
            );
            assert_eq!(cloned.message, "sample");
            assert_eq!(cloned.metadata_json, "{}");
        }
    }

    #[test]
    fn test_redact_credentials_replaces_values() {
        use super::ChannelStoreData;

        let mut creds = std::collections::HashMap::new();
        creds.insert(
            "TELEGRAM_BOT_TOKEN".to_string(),
            SecretValue::new(TEST_TELEGRAM_BOT_TOKEN.to_string()),
        );
        creds.insert("OTHER_SECRET".to_string(), SecretValue::new("s3cret"));
        let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

        let store = ChannelStoreData::new(
            1024 * 1024,
            &channel_name,
            ChannelCapabilities::default(),
            creds,
            Vec::new(),
            Arc::new(PairingStore::new()),
        );

        let error = format!(
            "HTTP request failed: error sending request for url \
            (https://api.telegram.org/bot{TEST_TELEGRAM_BOT_TOKEN}/getUpdates)"
        );

        let redacted = store.redact_credentials(&error);

        assert!(
            !redacted.contains(TEST_TELEGRAM_BOT_TOKEN),
            "credential value should be redacted"
        );
        assert!(
            redacted.contains("[REDACTED:TELEGRAM_BOT_TOKEN]"),
            "redacted text should contain placeholder name"
        );
        assert!(
            !redacted.contains("s3cret"),
            "other credentials should also be redacted"
        );
    }

    #[test]
    fn test_redact_credentials_no_op_without_credentials() {
        use super::ChannelStoreData;
        let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

        let store = ChannelStoreData::new(
            1024 * 1024,
            &channel_name,
            ChannelCapabilities::default(),
            std::collections::HashMap::new(),
            Vec::new(),
            Arc::new(PairingStore::new()),
        );

        let input = "some error message";
        assert_eq!(store.redact_credentials(input), input);
    }

    #[test]
    fn test_redact_credentials_url_encoded() {
        use super::{ChannelStoreData, ResolvedHostCredential};

        // Credential with characters that get URL-encoded
        let mut creds = std::collections::HashMap::new();
        creds.insert(
            "API_KEY".to_string(),
            SecretValue::new("key with spaces&special=chars"),
        );

        let host_creds = vec![ResolvedHostCredential {
            host_patterns: vec![
                HostPattern::new("api.example.com").expect("test host pattern is non-empty"),
            ],
            headers: std::collections::HashMap::new(),
            query_params: std::collections::HashMap::new(),
            secret_value: SecretValue::new("host secret+value"),
        }];
        let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

        let store = ChannelStoreData::new(
            1024 * 1024,
            &channel_name,
            ChannelCapabilities::default(),
            creds,
            host_creds,
            Arc::new(PairingStore::new()),
        );

        // Error containing URL-encoded form of the credential
        let error = "request failed: https://api.example.com?key=key%20with%20spaces%26special%3Dchars&host=host%20secret%2Bvalue";

        let redacted = store.redact_credentials(error);

        assert!(
            !redacted.contains("key%20with%20spaces"),
            "URL-encoded credential should be redacted, got: {}",
            redacted
        );
        assert!(
            !redacted.contains("host%20secret%2Bvalue"),
            "URL-encoded host credential should be redacted, got: {}",
            redacted
        );
    }

    #[test]
    fn test_redact_credentials_skips_empty_values() {
        use super::ChannelStoreData;

        let mut creds = std::collections::HashMap::new();
        creds.insert("EMPTY_TOKEN".to_string(), SecretValue::new(String::new()));
        let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

        let store = ChannelStoreData::new(
            1024 * 1024,
            &channel_name,
            ChannelCapabilities::default(),
            creds,
            Vec::new(),
            Arc::new(PairingStore::new()),
        );

        let input = "should not match anything";
        assert_eq!(store.redact_credentials(input), input);
    }

    /// Verify that WASM HTTP host functions work using a dedicated
    /// current-thread runtime inside spawn_blocking.
    #[tokio::test]
    async fn test_dedicated_runtime_inside_spawn_blocking() {
        let result = tokio::task::spawn_blocking(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build runtime");
            rt.block_on(async { 42 })
        })
        .await
        .expect("spawn_blocking panicked");
        assert_eq!(result, 42);
    }

    /// Verify a real HTTP request works using the dedicated-runtime pattern.
    /// This catches DNS, TLS, and I/O driver issues that trivial tests miss.
    #[tokio::test]
    #[ignore] // requires network
    async fn test_dedicated_runtime_real_http() {
        let result = tokio::task::spawn_blocking(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build runtime");
            rt.block_on(async {
                let client = reqwest::Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(10))
                    .build()
                    .expect("failed to build client");
                let resp = client
                    .get("https://api.telegram.org/bot000/getMe")
                    .timeout(std::time::Duration::from_secs(10))
                    .send()
                    .await;
                match resp {
                    Ok(r) => r.status().as_u16(),
                    Err(e) if e.is_timeout() => panic!("request timed out: {e}"),
                    Err(e) => panic!("unexpected error: {e}"),
                }
            })
        })
        .await
        .expect("spawn_blocking panicked");
        // 404 because "000" is not a valid bot token
        assert_eq!(result, 404);
    }

    #[tokio::test]
    async fn test_dispatch_emitted_messages_preserves_attachments() {
        use crate::channels::wasm::host::{Attachment, EmittedMessage};

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

        let rate_limiter = Arc::new(tokio::sync::RwLock::new(
            crate::channels::wasm::host::ChannelEmitRateLimiter::new(
                crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
            ),
        ));

        let attachments = vec![
            Attachment {
                id: "photo123".to_string(),
                mime_type: "image/jpeg".to_string(),
                filename: Some("cat.jpg".to_string()),
                size_bytes: Some(50_000),
                source_url: Some("https://api.telegram.org/file/photo123".to_string()),
                storage_key: None,
                extracted_text: None,
                data: Vec::new(),
                duration_secs: None,
            },
            Attachment {
                id: "doc456".to_string(),
                mime_type: "application/pdf".to_string(),
                filename: Some("report.pdf".to_string()),
                size_bytes: Some(120_000),
                source_url: None,
                storage_key: Some("store/doc456".to_string()),
                extracted_text: Some("Report contents...".to_string()),
                data: Vec::new(),
                duration_secs: None,
            },
        ];

        let messages =
            vec![EmittedMessage::new("user1", "Check these files").with_attachments(attachments)];

        let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
        let result = WasmChannel::dispatch_emitted_messages(
            "test-channel",
            messages,
            &message_tx,
            &rate_limiter,
            &last_broadcast_metadata,
            None,
        )
        .await;

        assert!(result.is_ok());

        let msg = rx.try_recv().expect("Should receive message");
        assert_eq!(msg.content, "Check these files");
        assert_eq!(msg.attachments.len(), 2);

        // Verify first attachment
        assert_eq!(msg.attachments[0].id, "photo123");
        assert_eq!(msg.attachments[0].mime_type, "image/jpeg");
        assert_eq!(msg.attachments[0].filename, Some("cat.jpg".to_string()));
        assert_eq!(msg.attachments[0].size_bytes, Some(50_000));
        assert_eq!(
            msg.attachments[0].source_url,
            Some("https://api.telegram.org/file/photo123".to_string())
        );

        // Verify second attachment
        assert_eq!(msg.attachments[1].id, "doc456");
        assert_eq!(msg.attachments[1].mime_type, "application/pdf");
        assert_eq!(
            msg.attachments[1].extracted_text,
            Some("Report contents...".to_string())
        );
        assert_eq!(
            msg.attachments[1].storage_key,
            Some("store/doc456".to_string())
        );
    }

    #[tokio::test]
    async fn test_dispatch_emitted_messages_no_attachments_backward_compat() {
        use crate::channels::wasm::host::EmittedMessage;

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

        let rate_limiter = Arc::new(tokio::sync::RwLock::new(
            crate::channels::wasm::host::ChannelEmitRateLimiter::new(
                crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
            ),
        ));

        let messages = vec![EmittedMessage::new("user1", "Just text, no attachments")];

        let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
        let result = WasmChannel::dispatch_emitted_messages(
            "test-channel",
            messages,
            &message_tx,
            &rate_limiter,
            &last_broadcast_metadata,
            None,
        )
        .await;

        assert!(result.is_ok());

        let msg = rx.try_recv().expect("Should receive message");
        assert_eq!(msg.content, "Just text, no attachments");
        assert!(msg.attachments.is_empty());
    }

    fn test_channel_http_capabilities(host: &str) -> ChannelCapabilities {
        use crate::tools::wasm::{Capabilities, EndpointPattern, HttpCapability};

        ChannelCapabilities::for_channel("test").with_tool_capabilities(
            Capabilities::default().with_http(HttpCapability::new(vec![
                EndpointPattern::host(host.to_string())
                    .with_path_prefix("/")
                    .with_methods(vec!["GET".to_string()]),
            ])),
        )
    }

    #[test]
    fn test_channel_http_request_allows_placeholder_header_injection() {
        use crate::channels::wasm::wrapper::ChannelStoreData;
        use crate::channels::wasm::wrapper::near::agent::channel_host;
        use std::collections::HashMap;

        let host = "slack.invalid";
        let slack_bot_token = "slack-dummy-token-12345".to_string();
        let mut credentials = HashMap::new();
        credentials.insert(
            "SLACK_BOT_TOKEN".to_string(),
            SecretValue::new(slack_bot_token),
        );
        let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

        let mut store = ChannelStoreData::new(
            1024 * 1024,
            &channel_name,
            test_channel_http_capabilities(host),
            credentials,
            Vec::new(),
            Arc::new(PairingStore::new()),
        );

        let err = <ChannelStoreData as channel_host::Host>::http_request(
            &mut store,
            "GET".to_string(),
            format!("https://{host}/api/chat.postMessage"),
            serde_json::json!({
                "Authorization": "Bearer {SLACK_BOT_TOKEN}",
                "Content-Type": "application/json"
            })
            .to_string(),
            None,
            Some(1000),
        )
        .expect_err("invalid public hostname should fail after request preparation");

        assert!(
            !err.contains("Potential secret leak blocked"),
            "placeholder-based auth header should progress past leak scanning, got: {err}"
        );
        assert!(
            err.contains("HTTP request failed") || err.contains("dns error"),
            "expected later-stage HTTP/DNS failure, got: {err}"
        );
    }

    #[test]
    fn test_mime_from_extension() {
        use super::mime_from_extension;
        assert_eq!(mime_from_extension("screenshot.png"), "image/png");
        assert_eq!(mime_from_extension("photo.JPG"), "image/jpeg");
        assert_eq!(mime_from_extension("photo.jpeg"), "image/jpeg");
        assert_eq!(mime_from_extension("animation.gif"), "image/gif");
        assert_eq!(mime_from_extension("doc.pdf"), "application/pdf");
        assert_eq!(mime_from_extension("video.mp4"), "video/mp4");
        assert_eq!(mime_from_extension("data.csv"), "text/csv");
        assert_eq!(
            mime_from_extension("unknown.qqqzzz"),
            "application/octet-stream"
        );
        assert_eq!(mime_from_extension("noext"), "application/octet-stream");
        assert_eq!(
            mime_from_extension("/home/user/.ironclaw/screenshot.png"),
            "image/png"
        );
    }
}
