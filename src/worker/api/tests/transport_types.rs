//! Transport type serialisation fidelity tests.

use rstest::rstest;

use crate::worker::api::{
    COMPLETE_ROUTE, CREDENTIALS_ROUTE, EVENT_ROUTE, JOB_ROUTE, PROMPT_ROUTE,
    REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_ROUTE, RemoteToolCatalogResponse,
    RemoteToolExecutionRequest, RemoteToolExecutionResponse, STATUS_ROUTE, TerminalResult,
};

use super::fixtures::{
    sample_catalog_response, sample_execution_request, sample_execution_response,
};

const fn const_str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }

    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }

    true
}

const _: () = assert!(const_str_eq(JOB_ROUTE, "/worker/{job_id}/job"));
const _: () = assert!(const_str_eq(
    CREDENTIALS_ROUTE,
    "/worker/{job_id}/credentials"
));
const _: () = assert!(const_str_eq(STATUS_ROUTE, "/worker/{job_id}/status"));
const _: () = assert!(const_str_eq(COMPLETE_ROUTE, "/worker/{job_id}/complete"));
const _: () = assert!(const_str_eq(EVENT_ROUTE, "/worker/{job_id}/event"));
const _: () = assert!(const_str_eq(PROMPT_ROUTE, "/worker/{job_id}/prompt"));
const _: () = assert!(const_str_eq(
    REMOTE_TOOL_CATALOG_ROUTE,
    "/worker/{job_id}/tools/catalog"
));
const _: () = assert!(const_str_eq(
    REMOTE_TOOL_EXECUTE_ROUTE,
    "/worker/{job_id}/tools/execute"
));

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

#[test]
fn terminal_result_round_trip_preserves_all_fields() {
    let result = TerminalResult::success("completed", Some(11));

    let serialized = serde_json::to_string(&result).expect("serialize TerminalResult");
    let deserialized: TerminalResult =
        serde_json::from_str(&serialized).expect("deserialize TerminalResult");

    assert_eq!(deserialized.success, result.success);
    assert_eq!(deserialized.message, result.message);
    assert_eq!(deserialized.iterations, result.iterations);
}

#[test]
fn terminal_result_omits_iterations_when_absent() {
    let serialized = serde_json::to_value(TerminalResult::failure("failed", None))
        .expect("serialize TerminalResult");

    assert_eq!(serialized["success"], false);
    assert_eq!(serialized["message"], "failed");
    assert!(
        serialized.get("iterations").is_none(),
        "iterations should be omitted when absent"
    );
}
