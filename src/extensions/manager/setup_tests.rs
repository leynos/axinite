//! Manager-level setup tests: declarations, readiness, and execution.
//!
//! The fixture installs a synthetic tool manifest with one credential and two
//! endpoints. Nothing contacts the endpoint's host; the planner is offline.

use std::sync::Arc;

use crate::extensions::setup::binding_test_support::{MemoryRepository, crypto};
use crate::extensions::setup::{
    BindingScope, CredentialName, EndpointBindingRepository, EndpointName, EndpointState, FieldKey,
    FieldKeyKind, SetupOperation,
};
use crate::extensions::{
    ExtensionError, ExtensionManager, ExtensionManagerConfig, FieldOutcome, NoOpDiscovery,
    NoOpMcpActivation, NoOpWasmChannelActivation, NoOpWasmToolActivation,
};
use crate::secrets::{InMemorySecretsStore, SecretsStore};
use crate::testing::credentials::TEST_CRYPTO_KEY;
use crate::tools::ToolRegistry;

/// Synthetic tool manifest: one secret and two endpoints, one optional.
const TOOL_MANIFEST: &str = r#"{
  "setup": {
    "fields": [
      {"type": "secret", "name": "token", "prompt": "API token"},
      {"type": "endpoint", "name": "calendar", "kind": "caldav", "prompt": "Calendar URL"},
      {"type": "endpoint", "name": "contacts", "kind": "carddav", "prompt": "Contacts URL", "optional": true}
    ]
  }
}"#;

/// Synthetic channel manifest with one auto-generated secret.
const CHANNEL_MANIFEST: &str = r#"{
  "name": "demo-channel",
  "setup": {
    "required_secrets": [
      {"name": "webhook_secret", "prompt": "Webhook signing secret for the demo channel integration", "auto_generate": {"length": 16}},
      {"name": "api_key", "prompt": "API key for the demo channel integration provider"}
    ]
  }
}"#;

struct Fixture {
    manager: ExtensionManager,
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    repository: Arc<MemoryRepository>,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    build_fixture("jmap", TOOL_MANIFEST)
}

fn channel_fixture() -> Fixture {
    build_fixture("demo-channel", CHANNEL_MANIFEST)
}

fn build_fixture(name: &str, manifest: &str) -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let tools_dir = dir.path().join("tools");
    let channels_dir = dir.path().join("channels");
    ambient_fs::create_dir_all(&tools_dir).expect("tools dir");
    ambient_fs::create_dir_all(&channels_dir).expect("channels dir");
    for target in [&tools_dir, &channels_dir] {
        ambient_fs::write(target.join(format!("{name}.wasm")), b"").expect("wasm marker");
        ambient_fs::write(
            target.join(format!("{name}.capabilities.json")),
            manifest.as_bytes(),
        )
        .expect("manifest");
    }

    let crypto = crypto(TEST_CRYPTO_KEY);
    let secrets: Arc<dyn SecretsStore + Send + Sync> =
        Arc::new(InMemorySecretsStore::new(Arc::clone(&crypto)));
    let repository = Arc::new(MemoryRepository::default());
    let bindings: Arc<dyn EndpointBindingRepository> = repository.clone();
    let manager = ExtensionManager::new(ExtensionManagerConfig {
        shared_state: crate::extensions::LiveWasmChannelSharedState::default(),
        discovery: Arc::new(NoOpDiscovery),
        relay_config: None,
        gateway_token: None,
        mcp_activation: Arc::new(NoOpMcpActivation),
        wasm_tool_activation: Arc::new(NoOpWasmToolActivation),
        wasm_channel_activation: Arc::new(NoOpWasmChannelActivation),
        mcp_clients: crate::extensions::McpClientsMap::default(),
        secrets: Arc::clone(&secrets),
        tool_registry: Arc::new(ToolRegistry::new()),
        hooks: None,
        wasm_tools_dir: tools_dir,
        wasm_channels_dir: channels_dir,
        tunnel_url: None,
        user_id: "test".to_string(),
        store: None,
        catalog_entries: Vec::new(),
        endpoint_bindings: Some(Arc::new(
            crate::extensions::setup::EndpointBindingService::new(bindings, crypto),
        )),
    });
    Fixture {
        manager,
        secrets,
        repository,
        _dir: dir,
    }
}

fn credential(name: &str) -> CredentialName {
    CredentialName::parse(name).expect("valid credential name")
}

fn endpoint(name: &str) -> EndpointName {
    EndpointName::parse(name).expect("valid endpoint name")
}

fn secret_op(name: &str, value: &str) -> SetupOperation {
    SetupOperation::Secret {
        name: credential(name),
        value: value.to_string(),
    }
}

fn endpoint_op(name: &str, url: &str) -> SetupOperation {
    SetupOperation::EndpointSet {
        name: endpoint(name),
        url: url.to_string(),
    }
}

fn clear_op(name: &str) -> SetupOperation {
    SetupOperation::EndpointClear {
        name: endpoint(name),
    }
}

fn assert_secret_is_stored(result: &crate::extensions::SetupApplication, name: &str) {
    let field = FieldKey::Secret(credential(name));
    assert!(
        result
            .outcomes()
            .iter()
            .any(|report| report.field() == &field && report.outcome() == FieldOutcome::Stored),
        "expected a stored outcome for {field}"
    );
}

#[tokio::test]
async fn a_mixed_submission_routes_each_field() {
    let fixture = fixture();
    let result = fixture
        .manager
        .apply_setup(
            "jmap",
            vec![
                secret_op("token", "s3cret"),
                endpoint_op("calendar", "https://mail.example.com/caldav"),
            ],
        )
        .await
        .expect("submission applies");

    assert_secret_is_stored(&result, "token");
    assert!(result.credentials_changed());
    assert!(result.snapshot().ready());
    assert_eq!(
        result.snapshot().endpoint_state(&endpoint("calendar")),
        Some(EndpointState::Configured)
    );
    assert!(!fixture.repository.stored().await.is_empty());
}

#[tokio::test]
async fn a_rejected_batch_writes_nothing() {
    let fixture = fixture();
    let error = fixture
        .manager
        .apply_setup(
            "jmap",
            vec![
                secret_op("token", "s3cret"),
                endpoint_op("calendar", "http://insecure.example.com/"),
            ],
        )
        .await
        .expect_err("insecure scheme is rejected");

    assert!(matches!(
        error,
        ExtensionError::SetupRejected {
            category: "endpoint_insecure_scheme",
            ..
        }
    ));
    assert!(
        !fixture
            .secrets
            .exists("test", "token")
            .await
            .expect("exists")
    );
    assert!(fixture.repository.stored().await.is_empty());
}

#[tokio::test]
async fn duplicate_operations_are_rejected() {
    let fixture = fixture();
    let error = fixture
        .manager
        .apply_setup(
            "jmap",
            vec![
                endpoint_op("calendar", "https://mail.example.com/caldav"),
                clear_op("calendar"),
            ],
        )
        .await
        .expect_err("duplicate operation");

    assert!(matches!(
        error,
        ExtensionError::SetupRejected {
            category: "duplicate_operation",
            ..
        }
    ));
}

#[tokio::test]
async fn a_missing_required_endpoint_blocks_readiness() {
    let fixture = fixture();
    let result = fixture
        .manager
        .apply_setup("jmap", vec![secret_op("token", "s3cret")])
        .await
        .expect("submission applies");

    assert!(!result.snapshot().ready());
    assert!(!result.activated());
    let blockers = result.snapshot().blockers();
    assert_eq!(blockers.len(), 1);
    assert_eq!(
        blockers[0].field(),
        &FieldKey::Endpoint(endpoint("calendar"))
    );
    assert!(result.message().contains("endpoint:calendar (missing)"));
}

#[tokio::test]
async fn a_corrupt_optional_endpoint_blocks_readiness() {
    let fixture = fixture();
    fixture
        .manager
        .apply_setup(
            "jmap",
            vec![
                secret_op("token", "s3cret"),
                endpoint_op("contacts", "https://mail.example.com/carddav"),
            ],
        )
        .await
        .expect("submission applies");
    let scope = BindingScope::new("test", "jmap");
    fixture
        .repository
        .mutate_row(&scope, "contacts", |record| {
            record.encrypted_value[0] ^= 0xff;
        })
        .await;

    let snapshot = fixture
        .manager
        .setup_snapshot("jmap")
        .await
        .expect("snapshot");
    assert_eq!(
        snapshot.endpoint_state(&endpoint("contacts")),
        Some(EndpointState::Corrupt)
    );
    assert!(
        !snapshot.ready(),
        "a corrupt optional endpoint still blocks"
    );
}

#[tokio::test]
async fn an_endpoint_only_edit_does_not_change_credentials() {
    let fixture = fixture();
    let result = fixture
        .manager
        .apply_setup(
            "jmap",
            vec![endpoint_op("calendar", "https://mail.example.com/caldav")],
        )
        .await
        .expect("submission applies");

    assert!(!result.credentials_changed());
    assert!(result.snapshot().secret_provided(&credential("token")) == Some(false));
}

#[tokio::test]
async fn a_blank_secret_keeps_the_stored_value() {
    let fixture = fixture();
    fixture
        .manager
        .apply_setup("jmap", vec![secret_op("token", "original")])
        .await
        .expect("first submission applies");
    let result = fixture
        .manager
        .apply_setup("jmap", vec![secret_op("token", "   ")])
        .await
        .expect("blank submission applies");

    let field = FieldKey::Secret(credential("token"));
    assert!(
        result
            .outcomes()
            .iter()
            .any(|report| report.field() == &field && report.outcome() == FieldOutcome::Kept)
    );
    let stored = fixture
        .secrets
        .get_decrypted("test", "token")
        .await
        .expect("decrypts");
    assert_eq!(stored.expose(), "original");
}

#[tokio::test]
async fn a_refused_write_is_indeterminate_and_later_effects_skip() {
    let fixture = fixture();
    fixture.repository.fail_writes();
    let result = fixture
        .manager
        .apply_setup(
            "jmap",
            vec![
                endpoint_op("calendar", "https://mail.example.com/caldav"),
                secret_op("token", "s3cret"),
            ],
        )
        .await
        .expect("the snapshot still reads");

    let outcomes: Vec<FieldOutcome> = result
        .outcomes()
        .iter()
        .map(|report| report.outcome())
        .collect();
    assert_eq!(
        outcomes,
        vec![FieldOutcome::Indeterminate, FieldOutcome::Skipped]
    );
    assert!(!result.snapshot().ready());
}

#[tokio::test]
async fn an_unreadable_store_is_an_error_not_an_absence() {
    let fixture = fixture();
    fixture.repository.fail_operations();
    let error = fixture
        .manager
        .setup_snapshot("jmap")
        .await
        .expect_err("a failed read is not readiness");

    assert!(matches!(
        error,
        ExtensionError::Other(message) if message.contains("storage unavailable")
    ));
}

#[tokio::test]
async fn a_channel_secret_is_auto_generated_at_its_declared_length() {
    let fixture = channel_fixture();
    let result = fixture
        .manager
        .apply_setup("demo-channel", vec![secret_op("api_key", "key")])
        .await
        .expect("submission applies");

    let generated = fixture
        .secrets
        .get_decrypted("test", "webhook_secret")
        .await
        .expect("generated");
    assert_eq!(generated.expose().len(), 32, "16 bytes hex-encoded");
    assert!(result.snapshot().ready());
    assert!(
        result
            .outcomes()
            .iter()
            .any(|report| report.field().kind() == FieldKeyKind::Secret
                && report.field().as_str() == "webhook_secret"
                && report.outcome() == FieldOutcome::Stored)
    );
}
