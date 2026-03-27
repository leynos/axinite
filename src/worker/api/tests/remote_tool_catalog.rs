//! Remote tool catalog fetching tests.

use rstest::rstest;
use uuid::Uuid;

use crate::error::WorkerError;
use crate::testing::credentials::TEST_BEARER_TOKEN;
use crate::worker::api::WorkerHttpClient;

use super::fixtures::{
    RemoteToolFailureRoute, RemoteToolFailureServerFactory, remote_tool_failure_server,
};

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_reports_non_success_statuses(
    remote_tool_failure_server: RemoteToolFailureServerFactory,
) -> anyhow::Result<()> {
    let server = remote_tool_failure_server(RemoteToolFailureRoute::Catalog).await?;

    let client = WorkerHttpClient::new(
        server.base_url.clone(),
        Uuid::new_v4(),
        TEST_BEARER_TOKEN.to_string(),
    );

    let err = client
        .get_remote_tool_catalog()
        .await
        .expect_err("catalog fetch should fail on non-success status");

    match err {
        WorkerError::OrchestratorRejected { reason, .. } => {
            assert!(reason.contains("GET /tools/catalog returned 403"));
        }
        other => panic!("unexpected worker error: {other:?}"),
    };

    Ok(())
}
