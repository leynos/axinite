//! Regression tests for proactive WASM schema publication during registration.

use chrono::Utc;
use uuid::Uuid;

use super::{ToolRegistry, WasmFromStorageRegistration};
use crate::llm::ToolDefinition;
use crate::testing::{github_wasm_artifact, metadata_test_runtime};
use crate::tools::wasm::storage::{NativeWasmToolStore, ToolKey};
use crate::tools::wasm::{
    StoredCapabilities, StoredWasmTool, StoredWasmToolWithBinary, ToolStatus, TrustLevel,
    WasmStorageError,
};

#[tokio::test]
async fn register_wasm_from_storage_publishes_guest_schema_when_storage_schema_is_null() {
    let registry = ToolRegistry::new();
    let runtime = metadata_test_runtime().expect("create metadata test runtime");
    let wasm_binary = github_wasm_bytes();
    let store = StubWasmToolStore::new(wasm_binary, serde_json::Value::Null);

    registry
        .register_wasm_from_storage(WasmFromStorageRegistration {
            store: &store,
            runtime: &runtime,
            user_id: "test-user",
            name: "github",
        })
        .await
        .expect("register wasm tool from storage");

    assert_real_github_schema(
        registry
            .tool_definitions()
            .await
            .into_iter()
            .find(|definition| definition.name == "github")
            .expect("github definition should be registered"),
    );
}

fn github_wasm_bytes() -> Vec<u8> {
    let wasm_path = github_wasm_artifact().expect("build or find github WASM artifact");
    std::fs::read(wasm_path).expect("read github wasm artifact")
}

fn assert_real_github_schema(definition: ToolDefinition) {
    assert_eq!(definition.parameters["type"], serde_json::json!("object"));
    assert_eq!(
        definition.parameters["properties"]["action"]["enum"][0],
        serde_json::json!("get_repo")
    );
    assert!(
        definition.parameters["required"]
            .as_array()
            .expect("required array")
            .iter()
            .any(|value| value == "action"),
        "expected required action field in schema: {}",
        definition.parameters
    );
}

struct StubWasmToolStore {
    tool: StoredWasmTool,
    wasm_binary: Vec<u8>,
}

impl StubWasmToolStore {
    fn new(wasm_binary: Vec<u8>, parameters_schema: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            tool: StoredWasmTool {
                id: Uuid::new_v4(),
                user_id: "test-user".to_string(),
                name: "github".to_string(),
                version: "0.1.0".to_string(),
                wit_version: crate::tools::wasm::WIT_TOOL_VERSION.to_string(),
                description: String::new(),
                parameters_schema,
                source_url: None,
                trust_level: TrustLevel::User,
                status: ToolStatus::Active,
                created_at: now,
                updated_at: now,
            },
            wasm_binary,
        }
    }
}

impl NativeWasmToolStore for StubWasmToolStore {
    async fn store(
        &self,
        _params: crate::tools::wasm::StoreToolParams,
    ) -> Result<StoredWasmTool, WasmStorageError> {
        Err(WasmStorageError::Database(
            "stub store does not support writes".to_string(),
        ))
    }

    async fn get(&self, _key: ToolKey<'_>) -> Result<StoredWasmTool, WasmStorageError> {
        Ok(self.tool.clone())
    }

    async fn get_with_binary(
        &self,
        _key: ToolKey<'_>,
    ) -> Result<StoredWasmToolWithBinary, WasmStorageError> {
        Ok(StoredWasmToolWithBinary {
            tool: self.tool.clone(),
            wasm_binary: self.wasm_binary.clone(),
            binary_hash: Vec::new(),
        })
    }

    async fn get_capabilities(
        &self,
        _tool_id: Uuid,
    ) -> Result<Option<StoredCapabilities>, WasmStorageError> {
        Ok(None)
    }

    async fn list(&self, _user_id: &str) -> Result<Vec<StoredWasmTool>, WasmStorageError> {
        Ok(vec![self.tool.clone()])
    }

    async fn update_status(
        &self,
        _key: ToolKey<'_>,
        _status: ToolStatus,
    ) -> Result<(), WasmStorageError> {
        Err(WasmStorageError::Database(
            "stub store does not support status updates".to_string(),
        ))
    }

    async fn delete(&self, _key: ToolKey<'_>) -> Result<bool, WasmStorageError> {
        Err(WasmStorageError::Database(
            "stub store does not support deletes".to_string(),
        ))
    }
}
