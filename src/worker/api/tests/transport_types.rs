//! Transport type serialization fidelity tests.

use rstest::rstest;

use crate::worker::api::{
    REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_ROUTE, RemoteToolCatalogResponse,
    RemoteToolExecutionRequest, RemoteToolExecutionResponse,
};

use super::fixtures::{
    sample_catalog_response, sample_execution_request, sample_execution_response,
};

#[test]
fn worker_and_orchestrator_share_remote_tool_route_constants() {
    assert_eq!(
        REMOTE_TOOL_CATALOG_ROUTE, "/worker/{job_id}/tools/catalog",
        "catalog route constant must match the expected orchestrator route"
    );
    assert_eq!(
        REMOTE_TOOL_EXECUTE_ROUTE, "/worker/{job_id}/tools/execute",
        "execute route constant must match the expected orchestrator route"
    );

    let test_job_id = "12345678-1234-1234-1234-123456789012";
    let catalog_route = REMOTE_TOOL_CATALOG_ROUTE.replace("{job_id}", test_job_id);
    let execute_route = REMOTE_TOOL_EXECUTE_ROUTE.replace("{job_id}", test_job_id);

    assert_eq!(
        catalog_route,
        format!("/worker/{}/tools/catalog", test_job_id),
        "catalog route must expand job_id parameter correctly"
    );
    assert_eq!(
        execute_route,
        format!("/worker/{}/tools/execute", test_job_id),
        "execute route must expand job_id parameter correctly"
    );
}

#[rstest]
fn remote_tool_catalog_response_round_trip_without_field_loss(
    sample_catalog_response: RemoteToolCatalogResponse,
) {
    let serialized = serde_json::to_string(&sample_catalog_response)
        .expect("serialize RemoteToolCatalogResponse");
    let deserialized: RemoteToolCatalogResponse =
        serde_json::from_str(&serialized).expect("deserialize RemoteToolCatalogResponse");

    assert_eq!(
        deserialized, sample_catalog_response,
        "catalog response must round-trip without field loss"
    );
}

#[rstest]
fn remote_tool_execution_request_round_trip_without_field_loss(
    sample_execution_request: RemoteToolExecutionRequest,
) {
    let serialized = serde_json::to_string(&sample_execution_request)
        .expect("serialize RemoteToolExecutionRequest");
    let deserialized: RemoteToolExecutionRequest =
        serde_json::from_str(&serialized).expect("deserialize RemoteToolExecutionRequest");

    assert_eq!(deserialized.tool_name, sample_execution_request.tool_name);
    assert_eq!(deserialized.params, sample_execution_request.params);
}

#[rstest]
fn remote_tool_execution_response_round_trip_without_field_loss(
    sample_execution_response: RemoteToolExecutionResponse,
) {
    let serialized = serde_json::to_string(&sample_execution_response)
        .expect("serialize RemoteToolExecutionResponse");
    let deserialized: RemoteToolExecutionResponse =
        serde_json::from_str(&serialized).expect("deserialize RemoteToolExecutionResponse");

    assert_eq!(
        deserialized.output.result,
        sample_execution_response.output.result
    );
    assert_eq!(
        deserialized.output.cost,
        sample_execution_response.output.cost
    );
    assert_eq!(
        deserialized.output.raw,
        sample_execution_response.output.raw
    );
    assert_eq!(
        deserialized.output.duration,
        sample_execution_response.output.duration
    );
}
