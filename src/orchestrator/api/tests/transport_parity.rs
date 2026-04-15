//! Serialisation round-trip tests for shared orchestrator–worker transport types.

use std::sync::Arc;

use super::super::remote_tools::hosted_remote_tool_catalog;
use super::fixtures::remote_tool_mocks::complex_tool_stub;
use crate::llm::ChatMessage;
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

#[tokio::test]
async fn worker_tool_completion_request_round_trips_through_shared_types() {
    let registry = Arc::new(ToolRegistry::new());
    registry.register(Arc::new(complex_tool_stub())).await;
    let (tools, _instructions, _version) = hosted_remote_tool_catalog(&registry).await;

    let completion_request = crate::worker::api::ProxyToolCompletionRequest {
        messages: vec![
            ChatMessage::system("Inspect the available tools."),
            ChatMessage::user("What can you do with the remote WASM tool?"),
        ],
        tools,
        model: Some("test-model".to_string()),
        max_tokens: Some(512),
        temperature: Some(0.1),
        tool_choice: Some("auto".to_string()),
    };

    let serialized = serde_json::to_string(&completion_request)
        .expect("serialize worker-built tool completion request");
    let deserialized: crate::worker::api::ProxyToolCompletionRequest =
        serde_json::from_str(&serialized)
            .expect("worker tool request must deserialize into shared type");

    assert_eq!(
        deserialized.messages.len(),
        completion_request.messages.len(),
        "tool completion request must preserve message count"
    );
    assert_eq!(
        deserialized.tools, completion_request.tools,
        "tool completion request must preserve the full advertised tool definitions"
    );
    assert_eq!(deserialized.model, completion_request.model);
    assert_eq!(deserialized.max_tokens, completion_request.max_tokens);
    assert_eq!(deserialized.temperature, completion_request.temperature);
    assert_eq!(deserialized.tool_choice, completion_request.tool_choice);
}
