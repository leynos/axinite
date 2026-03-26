//! Live WASM tool activation adapter.
//!
//! Loads a WASM tool from disk, registers it with the tool registry, and
//! optionally registers plugin hooks from the capabilities file.

use std::path::PathBuf;
use std::sync::Arc;

use crate::extensions::activation::NativeWasmToolActivationPort;
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};
use crate::hooks::HookRegistry;
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;
use crate::tools::wasm::{WasmToolLoader, WasmToolRuntime};

/// Live adapter wiring WASM tool activation to the real WASM runtime.
pub struct LiveWasmToolActivation {
    wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
    wasm_tools_dir: PathBuf,
    tool_registry: Arc<ToolRegistry>,
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    hooks: Option<Arc<HookRegistry>>,
}

impl LiveWasmToolActivation {
    pub fn new(
        wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
        wasm_tools_dir: PathBuf,
        tool_registry: Arc<ToolRegistry>,
        secrets: Arc<dyn SecretsStore + Send + Sync>,
        hooks: Option<Arc<HookRegistry>>,
    ) -> Self {
        Self {
            wasm_tool_runtime,
            wasm_tools_dir,
            tool_registry,
            secrets,
            hooks,
        }
    }
}

impl NativeWasmToolActivationPort for LiveWasmToolActivation {
    async fn activate_wasm_tool<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
        // Check if already active
        if self.tool_registry.has(name).await {
            return Ok(ActivateResult {
                name: name.to_string(),
                kind: ExtensionKind::WasmTool,
                tools_loaded: vec![name.to_string()],
                message: format!("WASM tool '{}' already active", name),
            });
        }

        let runtime = self.wasm_tool_runtime.as_ref().ok_or_else(|| {
            ExtensionError::ActivationFailed("WASM runtime not available".to_string())
        })?;

        let wasm_path = self.wasm_tools_dir.join(format!("{}.wasm", name));
        if !wasm_path.exists() {
            return Err(ExtensionError::NotInstalled(format!(
                "WASM tool '{}' not found at {}",
                name,
                wasm_path.display()
            )));
        }

        let cap_path = self
            .wasm_tools_dir
            .join(format!("{}.capabilities.json", name));
        let cap_path_option = if cap_path.exists() {
            Some(cap_path.as_path())
        } else {
            None
        };

        let loader = WasmToolLoader::new(Arc::clone(runtime), Arc::clone(&self.tool_registry))
            .with_secrets_store(Arc::clone(&self.secrets));
        loader
            .load_from_files(name, &wasm_path, cap_path_option)
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        if let Some(ref hooks) = self.hooks
            && let Some(cap_path) = cap_path_option
        {
            let source = format!("plugin.tool:{}", name);
            let registration =
                crate::hooks::bootstrap::register_plugin_bundle_from_capabilities_file(
                    hooks, &source, cap_path,
                )
                .await;

            if registration.total_registered() > 0 {
                tracing::info!(
                    extension = name,
                    hooks = registration.hooks,
                    outbound_webhooks = registration.outbound_webhooks,
                    "Registered plugin hooks for activated WASM tool"
                );
            }

            if registration.errors > 0 {
                tracing::warn!(
                    extension = name,
                    errors = registration.errors,
                    "Some plugin hooks failed to register"
                );
            }
        }

        tracing::info!("Activated WASM tool '{}'", name);

        Ok(ActivateResult {
            name: name.to_string(),
            kind: ExtensionKind::WasmTool,
            tools_loaded: vec![name.to_string()],
            message: format!("WASM tool '{}' loaded and ready", name),
        })
    }
}
