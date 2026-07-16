//! Loading of WASM tools from configured and dev build directories.

use std::sync::Arc;

use crate::config::Config;
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;
use crate::tools::wasm::WasmToolRuntime;

async fn scan_tools_dir(loader: &crate::tools::wasm::WasmToolLoader, tools_dir: &std::path::Path) {
    match loader.load_from_dir(tools_dir).await {
        Ok(results) => {
            if !results.loaded.is_empty() {
                tracing::debug!(
                    loaded = results.loaded.len(),
                    dir = %tools_dir.display(),
                    "Loaded WASM tools from directory"
                );
            }
            for (path, err) in results.errors {
                tracing::warn!(tool = %path.display(), error = %err, "Failed to load WASM tool");
            }
        }
        Err(e) => {
            tracing::warn!(dir = %tools_dir.display(), error = %e, "Failed to scan WASM tools directory");
        }
    }
}

async fn load_dev_wasm_tools(
    loader: &crate::tools::wasm::WasmToolLoader,
    tools_dir: &std::path::Path,
) -> Vec<String> {
    match crate::tools::wasm::load_dev_tools(loader, tools_dir).await {
        Ok(results) => {
            if !results.loaded.is_empty() {
                tracing::debug!(
                    loaded = results.loaded.len(),
                    "Loaded dev WASM tools from build artefacts"
                );
            }
            results.loaded
        }
        Err(e) => {
            tracing::debug!(error = %e, "No dev WASM tools found");
            Vec::new()
        }
    }
}

pub(super) async fn load_wasm_tools(
    config: &Config,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    tools: &Arc<ToolRegistry>,
    wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
) -> Vec<String> {
    let Some(runtime) = wasm_tool_runtime else {
        return Vec::new();
    };

    let mut loader = crate::tools::wasm::WasmToolLoader::new(
        std::sync::Arc::clone(&runtime),
        std::sync::Arc::clone(tools),
    );
    if let Some(ref s) = secrets_store {
        loader = loader.with_secrets_store(std::sync::Arc::clone(s));
    }

    scan_tools_dir(&loader, &config.wasm.tools_dir).await;
    load_dev_wasm_tools(&loader, &config.wasm.tools_dir).await
}
