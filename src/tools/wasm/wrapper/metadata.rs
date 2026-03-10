use wasmtime::Store;
use wasmtime::component::Linker;

use super::*;

/// Return the placeholder description used until real guest metadata is recovered.
pub(super) fn placeholder_description() -> String {
    "WASM sandboxed tool".to_string()
}

/// Return the placeholder schema used until real guest metadata is recovered.
pub(super) fn placeholder_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}

/// Maximum characters for the description portion of a tool hint.
const HINT_DESC_MAX: usize = 500;
/// Maximum characters for the schema portion of a tool hint.
const HINT_SCHEMA_MAX: usize = 3000;

impl WasmToolWrapper {
    /// Recover the guest-exported description and parameter schema.
    ///
    /// This method instantiates the component with the same linker, limits,
    /// and host wiring used for normal execution, then reads the pure
    /// `description()` and `schema()` guest exports. Registration uses the
    /// recovered pair to replace placeholder metadata before file-loaded WASM
    /// tools are exposed through `ToolRegistry::tool_definitions()`.
    ///
    /// # Returns
    ///
    /// Returns the guest-exported `(description, schema)` pair.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let wrapper = WasmToolWrapper::new(runtime, prepared, Capabilities::default());
    /// let (description, schema) = wrapper.exported_metadata()?;
    /// assert!(!description.is_empty());
    /// assert_eq!(schema["type"], serde_json::json!("object"));
    /// ```
    pub(crate) fn exported_metadata(&self) -> Result<(String, serde_json::Value), WasmError> {
        let engine = self.runtime.engine();
        let limits = &self.prepared.limits;

        let store_data = StoreData::new(
            limits.memory_bytes,
            self.capabilities.clone(),
            self.credentials.clone(),
            Vec::new(),
        );
        let mut store = Store::new(engine, store_data);

        if self.runtime.config().fuel_config.enabled {
            store
                .set_fuel(limits.fuel)
                .map_err(|e| WasmError::ConfigError(format!("Failed to set fuel: {}", e)))?;
        }

        store.epoch_deadline_trap();
        let ticks = (limits.timeout.as_millis() / EPOCH_TICK_INTERVAL.as_millis()).max(1) as u64;
        store.set_epoch_deadline(ticks);
        store.limiter(|data| &mut data.limiter);

        let component = self.prepared.component().clone();
        let mut linker = Linker::new(engine);
        Self::add_host_functions(&mut linker)?;

        let instance =
            SandboxedTool::instantiate(&mut store, &component, &linker).map_err(|e| {
                let msg = e.to_string();
                if msg.contains("near:agent") || msg.contains("import") {
                    WasmError::InstantiationFailed(format!(
                        "{msg}. This usually means the extension was compiled against \
                     a different WIT version than the host supports. \
                     Rebuild the extension against the current WIT (host: {}).",
                        crate::tools::wasm::WIT_TOOL_VERSION
                    ))
                } else {
                    WasmError::InstantiationFailed(msg)
                }
            })?;

        read_metadata_exports(instance.near_agent_tool(), &mut store)
    }
}

/// Read metadata directly from the guest's `description()` and `schema()` exports.
fn read_metadata_exports(
    tool_iface: &wit_tool::Guest,
    store: &mut Store<StoreData>,
) -> Result<(String, serde_json::Value), WasmError> {
    let description = tool_iface
        .call_description(&mut *store)
        .map_err(|e| WasmError::InstantiationFailed(e.to_string()))?;
    let schema_str = tool_iface
        .call_schema(&mut *store)
        .map_err(|e| WasmError::InstantiationFailed(e.to_string()))?;
    let schema = serde_json::from_str(&schema_str)
        .map_err(|e| WasmError::InvalidResponseJson(e.to_string()))?;
    Ok((description, schema))
}

/// Build a retry hint from the guest's `description()` and `schema()` exports.
pub(super) fn build_tool_hint(
    tool_iface: &wit_tool::Guest,
    store: &mut Store<StoreData>,
) -> String {
    let desc = tool_iface
        .call_description(&mut *store)
        .ok()
        .unwrap_or_default();
    let schema = tool_iface.call_schema(&mut *store).ok().unwrap_or_default();
    if desc.is_empty() && schema.is_empty() {
        return String::new();
    }
    let mut hint = String::new();
    if !desc.is_empty() {
        hint.push_str("Description: ");
        if desc.len() > HINT_DESC_MAX {
            let end = crate::util::floor_char_boundary(&desc, HINT_DESC_MAX);
            hint.push_str(&desc[..end]);
            hint.push('…');
        } else {
            hint.push_str(&desc);
        }
        hint.push('\n');
    }
    if !schema.is_empty() {
        hint.push_str("Parameters schema: ");
        if schema.len() > HINT_SCHEMA_MAX {
            let end = crate::util::floor_char_boundary(&schema, HINT_SCHEMA_MAX);
            hint.push_str(&schema[..end]);
            hint.push('…');
        } else {
            hint.push_str(&schema);
        }
    }
    hint
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use crate::tools::wasm::capabilities::Capabilities;
    use crate::tools::wasm::limits::ResourceLimits;
    use crate::tools::wasm::runtime::{WasmRuntimeConfig, WasmToolRuntime};

    use super::super::WasmToolWrapper;

    fn find_wasm_artifact(source_dir: &Path, crate_name: &str) -> Option<PathBuf> {
        let artifact_name = crate_name.replace('-', "_");

        {
            let target_triple = "wasm32-wasip2";
            let candidate = source_dir
                .join("target")
                .join(target_triple)
                .join("release")
                .join(format!("{artifact_name}.wasm"));
            if candidate.exists() {
                return Some(candidate);
            }
        }

        if let Ok(shared) = std::env::var("CARGO_TARGET_DIR") {
            {
                let target_triple = "wasm32-wasip2";
                let candidate = Path::new(&shared)
                    .join(target_triple)
                    .join("release")
                    .join(format!("{artifact_name}.wasm"));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        None
    }

    fn github_wasm_artifact() -> Option<PathBuf> {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        find_wasm_artifact(&repo_root.join("tools-src/github"), "github-tool")
    }

    fn metadata_test_runtime() -> Arc<WasmToolRuntime> {
        let config = WasmRuntimeConfig {
            default_limits: ResourceLimits::default()
                .with_memory(8 * 1024 * 1024)
                .with_fuel(100_000)
                .with_timeout(Duration::from_secs(5)),
            ..WasmRuntimeConfig::for_testing()
        };
        Arc::new(WasmToolRuntime::new(config).expect("create wasm runtime for metadata tests"))
    }

    #[tokio::test]
    async fn test_exported_metadata_from_real_github_component() {
        let Some(wasm_path) = github_wasm_artifact() else {
            eprintln!("Skipping exported metadata regression: github WASM artifact not built");
            return;
        };

        let runtime = metadata_test_runtime();
        let wasm_bytes = std::fs::read(&wasm_path).expect("read github wasm artifact");
        let prepared = runtime
            .prepare("github", &wasm_bytes, None)
            .await
            .expect("prepare github wasm component");
        let wrapper = WasmToolWrapper::new(runtime, prepared, Capabilities::default());

        let (description, schema) = wrapper
            .exported_metadata()
            .expect("extract exported metadata");

        assert!(
            description.contains("GitHub integration"),
            "expected real description, got: {description}"
        );
        assert_eq!(schema["type"], serde_json::json!("object"));
        assert!(
            schema["required"]
                .as_array()
                .expect("required array")
                .iter()
                .any(|value| value == "action"),
            "expected required action field in schema: {schema}"
        );
        let first_variant = schema["oneOf"]
            .as_array()
            .and_then(|variants| variants.first())
            .expect("oneOf variants");
        assert_eq!(
            first_variant["properties"]["action"]["const"],
            serde_json::json!("get_repo")
        );
        assert_eq!(
            first_variant["properties"]["owner"]["type"],
            serde_json::json!("string")
        );
    }
}
