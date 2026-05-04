//! Regression tests for proactive WASM schema publication during registration.

use std::sync::Arc;

use chrono::Utc;
use rstest::rstest;
use uuid::Uuid;

use super::{ToolRegistry, WasmFromStorageRegistration, WasmToolRegistration};
use crate::llm::ToolDefinition;
use crate::secrets::CredentialMapping;
use crate::testing::credentials::test_secrets_store;
use crate::testing::{github_wasm_artifact, metadata_test_runtime};
use crate::tools::wasm::storage::{NativeWasmToolStore, ToolKey};
use crate::tools::wasm::{
    Capabilities, HttpCapability, SharedCredentialRegistry, StoredCapabilities, StoredWasmTool,
    StoredWasmToolWithBinary, ToolStatus, TrustLevel, WasmError, WasmStorageError,
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
    // Null or placeholder schemas trigger guest export recovery.
    let registry = ToolRegistry::new();
    let runtime = metadata_test_runtime().expect("create metadata test runtime");
    let wasm_binary = github_wasm_bytes();

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

    let definition = registry
        .tool_definitions()
        .await
        .into_iter()
        .find(|definition| definition.name == "github")
        .expect("github definition should be registered");

    assert_real_github_schema(definition);
}

#[tokio::test]
async fn register_wasm_persists_credentials_only_after_successful_registration() {
    let credential_registry = Arc::new(SharedCredentialRegistry::new());
    let registry = ToolRegistry::new().with_credentials(
        Arc::clone(&credential_registry),
        Arc::new(test_secrets_store()),
    );
    let runtime = metadata_test_runtime().expect("create metadata test runtime");
    let wasm_binary = github_wasm_bytes();

    let rejected = registry
        .register_wasm(registration_with_credential(
            "http",
            &wasm_binary,
            &runtime,
            CredentialSpec {
                secret_name: "rejected_token",
                host_pattern: "rejected.example.com",
            },
        ))
        .await;
    assert!(
        matches!(rejected, Err(WasmError::ConfigError(message)) if message == "tool registration rejected"),
        "protected tool name should reject registration"
    );
    assert!(
        credential_registry
            .find_for_host("rejected.example.com")
            .is_empty(),
        "rejected registrations must not publish credential mappings"
    );

    registry
        .register_wasm(registration_with_credential(
            "github",
            &wasm_binary,
            &runtime,
            CredentialSpec {
                secret_name: "accepted_token",
                host_pattern: "api.example.com",
            },
        ))
        .await
        .expect("successful registration should persist credentials");

    let mappings = credential_registry.find_for_host("api.example.com");
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].secret_name, "accepted_token");
    registry
        .register_wasm(registration_with_credential(
            "github",
            &wasm_binary,
            &runtime,
            CredentialSpec {
                secret_name: "rotated_token",
                host_pattern: "api-v2.example.com",
            },
        ))
        .await
        .expect("re-registration should replace credentials for the same tool");
    assert!(
        credential_registry
            .find_for_host("api.example.com")
            .is_empty(),
        "re-registration should remove stale credential mappings for the tool"
    );
    let mappings = credential_registry.find_for_host("api-v2.example.com");
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].secret_name, "rotated_token");
    assert!(
        credential_registry
            .find_for_host("rejected.example.com")
            .is_empty(),
        "failed registrations should leave the credential registry unchanged"
    );
}

fn github_wasm_bytes() -> Vec<u8> {
    let wasm_path = github_wasm_artifact().expect("build or find github WASM artifact");
    std::fs::read(wasm_path).expect("read github wasm artifact")
}

struct CredentialSpec<'a> {
    secret_name: &'a str,
    host_pattern: &'a str,
}

fn registration_with_credential<'a>(
    name: &'a str,
    wasm_bytes: &'a [u8],
    runtime: &'a Arc<crate::tools::wasm::WasmToolRuntime>,
    credential: CredentialSpec<'_>,
) -> WasmToolRegistration<'a> {
    WasmToolRegistration {
        name,
        wasm_bytes,
        runtime,
        capabilities: capabilities_with_credential(credential.secret_name, credential.host_pattern),
        limits: None,
        description: Some("Credential persistence test tool"),
        schema: Some(serde_json::json!({
            "type": "object",
            "properties": {}
        })),
        secrets_store: None,
        oauth_refresh: None,
    }
}

fn capabilities_with_credential(secret_name: &str, host_pattern: &str) -> Capabilities {
    Capabilities::default().with_http(HttpCapability::default().with_credential(
        secret_name,
        CredentialMapping::bearer(secret_name, host_pattern),
    ))
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
