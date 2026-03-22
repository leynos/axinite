//! Placeholder metadata defaults, guest export recovery, and tool-hint helpers
//! for WASM tool wrappers.
//!
//! This module centralises the metadata path used while a wrapper is being
//! constructed: placeholder description/schema values, recovery of the guest's
//! exported `description()` and `schema()`, and generation of compact retry
//! hints for schema-aware failures.

use wasmtime::Store;
use wasmtime::component::Linker;
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

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
    /// This method instantiates the component with a metadata-only host linker
    /// and minimal store state, then reads the pure `description()` and
    /// `schema()` guest exports. Registration uses the recovered pair to
    /// replace placeholder metadata before file-loaded WASM tools are exposed
    /// through `ToolRegistry::tool_definitions()`.
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

        let store_data = MetadataStoreData::new(limits.memory_bytes);
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
        add_metadata_host_functions(&mut linker)?;

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

struct MetadataStoreData {
    limiter: WasmResourceLimiter,
    wasi: WasiCtx,
    table: ResourceTable,
}

impl MetadataStoreData {
    fn new(memory_limit: u64) -> Self {
        Self {
            limiter: WasmResourceLimiter::new(memory_limit),
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
        }
    }
}

impl WasiView for MetadataStoreData {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl near::agent::host::Host for MetadataStoreData {
    fn log(&mut self, _level: near::agent::host::LogLevel, _message: String) {}

    fn now_millis(&mut self) -> u64 {
        0
    }

    fn workspace_read(&mut self, _path: String) -> Option<String> {
        None
    }

    fn http_request(
        &mut self,
        _method: String,
        _url: String,
        _headers_json: String,
        _body: Option<Vec<u8>>,
        _timeout_ms: Option<u32>,
    ) -> Result<near::agent::host::HttpResponse, String> {
        Err("metadata export context does not permit http_request".to_string())
    }

    fn tool_invoke(&mut self, _alias: String, _params_json: String) -> Result<String, String> {
        Err("metadata export context does not permit tool_invoke".to_string())
    }

    fn secret_exists(&mut self, _name: String) -> bool {
        false
    }
}

fn add_metadata_host_functions(linker: &mut Linker<MetadataStoreData>) -> Result<(), WasmError> {
    wasmtime_wasi::add_to_linker_sync(linker)
        .map_err(|e| WasmError::ConfigError(format!("Failed to add WASI functions: {}", e)))?;
    near::agent::host::add_to_linker(linker, |state| state)
        .map_err(|e| WasmError::ConfigError(format!("Failed to add host functions: {}", e)))?;
    Ok(())
}

/// Read metadata strings directly from the guest's `description()` and
/// `schema()` exports.
fn exported_metadata_strings<T>(
    tool_iface: &wit_tool::Guest,
    store: &mut Store<T>,
) -> Result<(String, String), WasmError>
where
    T: WasiView + near::agent::host::Host,
{
    let description = tool_iface
        .call_description(&mut *store)
        .map_err(|e| WasmError::InstantiationFailed(e.to_string()))?;
    let schema = tool_iface
        .call_schema(&mut *store)
        .map_err(|e| WasmError::InstantiationFailed(e.to_string()))?;
    Ok((description, schema))
}

/// Read metadata directly from the guest's `description()` and `schema()`
/// exports.
fn read_metadata_exports<T>(
    tool_iface: &wit_tool::Guest,
    store: &mut Store<T>,
) -> Result<(String, serde_json::Value), WasmError>
where
    T: WasiView + near::agent::host::Host,
{
    let (description, schema_str) = exported_metadata_strings(tool_iface, store)?;
    let schema = serde_json::from_str(&schema_str)
        .map_err(|e| WasmError::InvalidResponseJson(e.to_string()))?;
    Ok((description, schema))
}

/// Build a retry hint from the guest's `description()` and `schema()` exports.
pub(super) fn build_tool_hint(
    tool_iface: &wit_tool::Guest,
    store: &mut Store<StoreData>,
) -> String {
    let (desc, schema) = exported_metadata_strings(tool_iface, store)
        .ok()
        .unwrap_or_default();
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
    use rstest::{fixture, rstest};

    use crate::testing::{github_wasm_artifact, metadata_test_runtime};
    use crate::tools::Tool;
    use crate::tools::tool::HostedToolCatalogSource;
    use crate::tools::wasm::capabilities::Capabilities;

    use super::super::WasmToolWrapper;

    #[fixture]
    async fn github_wrapper() -> WasmToolWrapper {
        let wasm_path = github_wasm_artifact().expect("build or find github WASM artifact");

        let runtime = metadata_test_runtime().expect("create metadata test runtime");
        let wasm_bytes = std::fs::read(&wasm_path).expect("read github wasm artifact");
        let prepared = runtime
            .prepare("github", &wasm_bytes, None)
            .await
            .expect("prepare github wasm component");
        WasmToolWrapper::new(runtime, prepared, Capabilities::default())
    }

    #[rstest]
    #[tokio::test]
    async fn test_exported_metadata_from_real_github_component(
        #[future] github_wrapper: WasmToolWrapper,
    ) {
        let wrapper = github_wrapper.await;

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
        assert!(
            schema.get("oneOf").is_none(),
            "top-level oneOf should not be exported for OpenAI compatibility: {schema}"
        );
        assert_eq!(
            schema["properties"]["action"]["enum"][0],
            serde_json::json!("get_repo")
        );
        assert_eq!(
            schema["properties"]["owner"]["type"],
            serde_json::json!("string")
        );
    }

    #[rstest]
    #[tokio::test]
    async fn wasm_tool_wrapper_reports_wasm_catalog_source(
        #[future] github_wrapper: WasmToolWrapper,
    ) {
        let wrapper = github_wrapper.await;

        assert_eq!(
            wrapper.hosted_tool_catalog_source(),
            Some(HostedToolCatalogSource::Wasm)
        );
    }
}
