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
    // These constants are declared in worker::api::types but are shared with the
    // orchestrator. The orchestrator imports them from worker::api to ensure both
    // sides use identical route paths for hosted remote-tool operations.

    // Verify the raw route constants contain the expected placeholder and path
    // segments so route parity holds by construction, not just by expansion.
    assert!(
        REMOTE_TOOL_CATALOG_ROUTE.contains("{job_id}"),
        "catalog route constant must contain the {{job_id}} placeholder"
    );
    assert!(
        REMOTE_TOOL_CATALOG_ROUTE.contains("/worker/"),
        "catalog route constant must include the /worker/ prefix"
    );
    assert!(
        REMOTE_TOOL_CATALOG_ROUTE.contains("/tools/catalog"),
        "catalog route constant must include the /tools/catalog segment"
    );
    assert!(
        REMOTE_TOOL_EXECUTE_ROUTE.contains("{job_id}"),
        "execute route constant must contain the {{job_id}} placeholder"
    );
    assert!(
        REMOTE_TOOL_EXECUTE_ROUTE.contains("/worker/"),
        "execute route constant must include the /worker/ prefix"
    );
    assert!(
        REMOTE_TOOL_EXECUTE_ROUTE.contains("/tools/execute"),
        "execute route constant must include the /tools/execute segment"
    );

    let job_id = "12345678-1234-1234-1234-123456789012";

    let catalog_route = REMOTE_TOOL_CATALOG_ROUTE.replace("{job_id}", job_id);
    let execute_route = REMOTE_TOOL_EXECUTE_ROUTE.replace("{job_id}", job_id);

    assert_eq!(
        catalog_route,
        format!("/worker/{}/tools/catalog", job_id),
        "catalog route must expand job_id parameter correctly"
    );
    assert_eq!(
        execute_route,
        format!("/worker/{}/tools/execute", job_id),
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

    assert_eq!(
        deserialized, sample_execution_request,
        "execution request must round-trip without field loss"
    );
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
        deserialized, sample_execution_response,
        "execution response must round-trip without field loss"
    );
}
