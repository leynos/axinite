//! Serialisation round-trip tests for shared orchestrator–worker transport types.

use std::sync::Arc;

use super::super::remote_tools::hosted_remote_tool_catalog;
use super::fixtures::remote_tool_mocks::complex_tool_stub;
use crate::tools::ToolRegistry;

#[tokio::test]
async fn orchestrator_catalog_response_round_trips_through_worker_shared_types() {
    let registry = Arc::new(ToolRegistry::new());
    registry.register(Arc::new(complex_tool_stub())).await;

    let (tools, instructions, version) = hosted_remote_tool_catalog(&registry).await;

    let catalog_response = crate::worker::api::RemoteToolCatalogResponse {
        tools: tools.clone(),
        toolset_instructions: instructions.clone(),
        catalog_version: version,
    };

    let serialized = serde_json::to_string(&catalog_response)
        .expect("serialize orchestrator-built catalog response");
    let deserialized: crate::worker::api::RemoteToolCatalogResponse =
        serde_json::from_str(&serialized)
            .expect("orchestrator response must deserialize into shared type");

    assert_eq!(
        deserialized, catalog_response,
        "catalog response must round-trip without field loss"
    );
}

#[tokio::test]
async fn worker_execution_request_round_trips_through_shared_types() {
    let execution_request = crate::worker::api::RemoteToolExecutionRequest {
        tool_name: "remote_tool_fidelity_fixture".to_string(),
        params: serde_json::json!({"query": "test", "options": {"limit": 10}}),
    };

    let serialized = serde_json::to_string(&execution_request)
        .expect("serialize worker-built execution request");
    let deserialized: crate::worker::api::RemoteToolExecutionRequest =
        serde_json::from_str(&serialized)
            .expect("worker request must deserialize into shared type");

    assert_eq!(
        deserialized, execution_request,
        "execution request must round-trip without field loss"
    );
}

#[tokio::test]
async fn orchestrator_execution_response_round_trips_through_worker_shared_types() {
    let execution_response = crate::worker::api::RemoteToolExecutionResponse {
        output: crate::tools::ToolOutput::success(
            serde_json::json!({"result": "executed"}),
            std::time::Duration::from_millis(15),
        )
        .with_cost(rust_decimal::Decimal::new(200, 2))
        .with_raw("orchestrator tool output"),
    };

    let serialized = serde_json::to_string(&execution_response)
        .expect("serialize orchestrator-built execution response");
    let deserialized: crate::worker::api::RemoteToolExecutionResponse =
        serde_json::from_str(&serialized)
            .expect("orchestrator response must deserialize into shared type");

    assert_eq!(
        deserialized, execution_response,
        "execution response must round-trip without field loss"
    );
}
