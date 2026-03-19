//! Error mapping helpers for worker-side HTTP transport.

use crate::error::WorkerError;

pub(super) async fn map_remote_tool_status(resp: reqwest::Response) -> WorkerError {
    let status = resp.status();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(std::time::Duration::from_secs);
    let body = resp.text().await.unwrap_or_default();
    let reason = format!(
        "Remote tool execution: orchestrator returned {}: {}",
        status, body
    );

    match status {
        reqwest::StatusCode::BAD_REQUEST => WorkerError::BadRequest { reason },
        reqwest::StatusCode::FORBIDDEN => WorkerError::Unauthorized { reason },
        reqwest::StatusCode::TOO_MANY_REQUESTS => WorkerError::RateLimited {
            reason,
            retry_after,
        },
        reqwest::StatusCode::BAD_GATEWAY => WorkerError::BadGateway { reason },
        _ => WorkerError::LlmProxyFailed { reason },
    }
}
