//! Regression tests for proactive WASM schema publication during registration.

use chrono::Utc;
use rstest::rstest;
use uuid::Uuid;

use super::{ToolRegistry, WasmFromStorageRegistration};
use crate::llm::ToolDefinition;
use crate::testing::{github_wasm_artifact, metadata_test_runtime};
use crate::tools::wasm::storage::{NativeWasmToolStore, ToolKey};
use crate::tools::wasm::{
    StoredCapabilities, StoredWasmTool, StoredWasmToolWithBinary, ToolStatus, TrustLevel,
    WasmStorageError,
};

#[rstest]
#[case(
    serde_json::from_str(&crate::tools::wasm::placeholder_json()).expect("parse placeholder JSON")
)]
#[case(serde_json::Value::Null)]
#[tokio::test]
async fn register_wasm_from_storage_recovers_guest_schema_for_absent_or_placeholder_schema(
    #[case] parameters_schema: serde_json::Value,
) {
    // Verify that when the stored schema is Null or the placeholder JSON,
    // normalized_schema returns None and the loader triggers guest export recovery.
    let registry = ToolRegistry::new();
    let runtime = metadata_test_runtime().expect("create metadata test runtime");
    let wasm_binary = github_wasm_bytes();

    // Verify the schema normalizes to None (this is the key assertion)
    assert!(
        super::schema::normalized_schema(parameters_schema.clone()).is_none(),
        "schema should normalize to None to trigger recovery"
    );

    let store = StubWasmToolStore::new(wasm_binary, parameters_schema);

    registry
        .register_wasm_from_storage(WasmFromStorageRegistration {
            store: &store,
            runtime: &runtime,
            user_id: "test-user",
            name: "github",
        })
        .await
        .expect("register wasm tool from storage");

    // Verify the tool was registered with the real guest-exported schema
    // (not the placeholder/empty), indicating recovery was triggered
    let definition = registry
        .tool_definitions()
        .await
        .into_iter()
        .find(|definition| definition.name == "github")
        .expect("github definition should be registered");

    // Assert we got the real GitHub schema (with actual properties),
    // not the placeholder empty object schema
    assert_real_github_schema(definition);
}

fn github_wasm_bytes() -> Vec<u8> {
    let wasm_path = github_wasm_artifact().expect("build or find github WASM artifact");
    std::fs::read(wasm_path).expect("read github wasm artifact")
}

fn assert_real_github_schema(definition: ToolDefinition) {
    crate::testing::github::assert_real_github_schema(&definition.parameters);
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

    async fn get(&self, key: ToolKey<'_>) -> Result<StoredWasmTool, WasmStorageError> {
        if key.user_id != self.tool.user_id || key.name != self.tool.name {
            return Err(WasmStorageError::NotFound(format!(
                "tool not found for user_id={}, name={}",
                key.user_id, key.name
            )));
        }
        Ok(self.tool.clone())
    }

    async fn get_with_binary(
        &self,
        key: ToolKey<'_>,
    ) -> Result<StoredWasmToolWithBinary, WasmStorageError> {
        if key.user_id != self.tool.user_id || key.name != self.tool.name {
            return Err(WasmStorageError::NotFound(format!(
                "tool not found for user_id={}, name={}",
                key.user_id, key.name
            )));
        }
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
