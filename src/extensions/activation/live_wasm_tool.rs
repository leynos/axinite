//! Live WASM tool activation adapter.
//!
//! Loads a WASM tool from disk, registers it with the tool registry, and
//! optionally registers plugin hooks from the capabilities file.

use std::path::PathBuf;
use std::sync::Arc;

use crate::extensions::activation::{ActivationFuture, WasmToolActivationPort};
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

/// Configuration for [`LiveWasmToolActivation`].
pub struct LiveWasmToolActivationConfig {
    /// WASM runtime used to load tools. `None` makes activation fail with a
    /// "runtime not available" error.
    pub wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
    /// Directory holding `<name>.wasm` files and optional `<name>.capabilities.json` files.
    pub wasm_tools_dir: PathBuf,
    /// Registry that receives the loaded tool; also consulted to detect an already-active tool.
    pub tool_registry: Arc<ToolRegistry>,
    /// Secrets store handed to the loader for the tool's credentials.
    pub secrets: Arc<dyn SecretsStore + Send + Sync>,
    /// Hook registry for plugin hooks declared in the capabilities file. `None` skips
    /// hook registration.
    pub hooks: Option<Arc<HookRegistry>>,
}

impl LiveWasmToolActivation {
    /// Create the adapter from `config`. Nothing is loaded until activation is requested.
    pub fn new(config: LiveWasmToolActivationConfig) -> Self {
        Self {
            wasm_tool_runtime: config.wasm_tool_runtime,
            wasm_tools_dir: config.wasm_tools_dir,
            tool_registry: config.tool_registry,
            secrets: config.secrets,
            hooks: config.hooks,
        }
    }

    fn resolve_cap_path(&self, name: &str) -> Option<PathBuf> {
        let path = self
            .wasm_tools_dir
            .join(format!("{}.capabilities.json", name));
        path.exists().then_some(path)
    }

    async fn register_hooks_for_tool(
        &self,
        hooks: &Arc<HookRegistry>,
        name: &str,
        cap_path: &std::path::Path,
    ) {
        let source = format!("plugin.tool:{}", name);
        let registration = crate::hooks::bootstrap::register_plugin_bundle_from_capabilities_file(
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
}

impl WasmToolActivationPort for LiveWasmToolActivation {
    fn activate_wasm_tool<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(async move { self.activate_wasm_tool_inner(name).await })
    }
}

impl LiveWasmToolActivation {
    async fn activate_wasm_tool_inner<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
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

        let cap_path = self.resolve_cap_path(name);

        let loader = WasmToolLoader::new(Arc::clone(runtime), Arc::clone(&self.tool_registry))
            .with_secrets_store(Arc::clone(&self.secrets));
        loader
            .load_from_files(name, &wasm_path, cap_path.as_deref())
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        if let (Some(hooks), Some(cap)) = (&self.hooks, &cap_path) {
            self.register_hooks_for_tool(hooks, name, cap).await;
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
