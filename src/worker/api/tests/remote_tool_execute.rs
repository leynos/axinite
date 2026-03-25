//! Remote tool execution tests.

use rstest::rstest;
use uuid::Uuid;

use crate::error::WorkerError;
use crate::testing::credentials::TEST_BEARER_TOKEN;
use crate::worker::api::WorkerHttpClient;

use super::fixtures::{remote_tool_failure_server, RemoteToolFailureRoute, RemoteToolFailureServerFactory};

#[rstest]
#[case(RemoteToolFailureRoute::ExecuteBadRequest, "bad params")]
#[case(RemoteToolFailureRoute::ExecuteForbidden, "approval required")]
#[case(RemoteToolFailureRoute::ExecuteRateLimited, "slow down")]
#[case(RemoteToolFailureRoute::ExecuteBadGateway, "proxy failure")]
#[case(RemoteToolFailureRoute::ExecuteInternalError, "remote tool blew up")]
#[tokio::test]
async fn remote_tool_execute_preserves_non_success_statuses(
    remote_tool_failure_server: RemoteToolFailureServerFactory,
    #[case] route: RemoteToolFailureRoute,
    #[case] expected_message: &str,
) {
    let server = remote_tool_failure_server(route).await;

    let client = WorkerHttpClient::new(
        server.base_url,
        Uuid::new_v4(),
        TEST_BEARER_TOKEN.to_string(),
    );

    let err = client
        .execute_remote_tool("github_search", &serde_json::json!({"query": 7}))
        .await
        .expect_err("remote-tool execute should fail on non-success status");

    match (route, err) {
        (RemoteToolFailureRoute::ExecuteBadRequest, WorkerError::BadRequest { reason }) => {
            assert!(reason.contains(expected_message))
        }
        (RemoteToolFailureRoute::ExecuteForbidden, WorkerError::Unauthorized { reason }) => {
            assert!(reason.contains(expected_message))
        }
        (
            RemoteToolFailureRoute::ExecuteRateLimited,
            WorkerError::RateLimited {
                reason,
                retry_after: Some(retry_after),
            },
        ) => {
            assert!(reason.contains(expected_message));
            assert_eq!(retry_after, std::time::Duration::from_secs(7));
        }
        (RemoteToolFailureRoute::ExecuteBadGateway, WorkerError::BadGateway { reason }) => {
            assert!(reason.contains(expected_message))
        }
        (
            RemoteToolFailureRoute::ExecuteInternalError,
            WorkerError::RemoteToolFailed { reason },
        ) => {
            assert!(reason.contains(expected_message))
        }
        (_, other) => panic!("unexpected worker error: {other:?}"),
    }

    server.handle.abort();
    let _ = server.handle.await;
}
