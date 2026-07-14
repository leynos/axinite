//! Unit tests for the time tool's timezone handling.

use super::*;

#[tokio::test]
async fn test_now_accepts_explicit_timezone() {
    let tool = TimeTool;
    let ctx = JobContext::with_user("test", "chat", "test");

    let output = tool
        .execute(
            serde_json::json!({
                "operation": "now",
                "timezone": "America/New_York"
            }),
            &ctx,
        )
        .await
        .expect("execute");

    assert_eq!(output.result["timezone"].as_str(), Some("America/New_York"));
    assert!(
        output.result.get("utc_iso").is_some(),
        "should have utc_iso"
    );
    assert!(
        output.result.get("local_iso").is_some(),
        "should have local_iso"
    );
}

#[tokio::test]
async fn test_now_includes_local_time_when_user_timezone_set() {
    let tool = TimeTool;
    let mut ctx = JobContext::with_user("test", "chat", "test");
    ctx.user_timezone = "America/New_York".to_string();

    let output = tool
        .execute(serde_json::json!({"operation": "now"}), &ctx)
        .await
        .expect("execute");
    assert!(
        output.result.get("local_iso").is_some(),
        "should have local_iso"
    );
    assert_eq!(
        output.result["timezone"].as_str(),
        Some("America/New_York"),
        "should report timezone"
    );
}

#[tokio::test]
async fn test_now_uses_context_metadata_timezone_fallback() {
    let tool = TimeTool;
    let mut ctx = JobContext::with_user("test", "chat", "test");
    ctx.metadata = serde_json::json!({
        "user_timezone": "America/Los_Angeles"
    });

    let output = tool
        .execute(serde_json::json!({"operation": "now"}), &ctx)
        .await
        .expect("execute");

    assert_eq!(
        output.result["timezone"].as_str(),
        Some("America/Los_Angeles")
    );
    assert!(
        output.result.get("local_iso").is_some(),
        "should have local_iso"
    );
}

#[tokio::test]
async fn test_now_returns_utc_by_default() {
    let tool = TimeTool;
    let ctx = JobContext::with_user("test", "chat", "test");
    // Default user_timezone is "UTC" -- context_timezone skips UTC so no
    // local_iso is added, but iso and utc_iso are always present.
    let output = tool
        .execute(serde_json::json!({"operation": "now"}), &ctx)
        .await
        .expect("execute");
    assert!(output.result.get("iso").is_some(), "should have iso");
}

#[tokio::test]
async fn test_convert_across_dst_boundary() {
    let tool = TimeTool;
    let ctx = JobContext::with_user("test", "chat", "test");

    let output = tool
        .execute(
            serde_json::json!({
                "operation": "convert",
                "input": "2026-03-08T07:30:00Z",
                "to_timezone": "America/New_York"
            }),
            &ctx,
        )
        .await
        .expect("execute");

    assert_eq!(output.result["timezone"].as_str(), Some("America/New_York"));
    assert_eq!(
        output.result["output"].as_str(),
        Some("2026-03-08T03:30:00-04:00")
    );
}

#[tokio::test]
async fn test_format_with_timezone() {
    let tool = TimeTool;
    let ctx = JobContext::with_user("test", "chat", "test");

    let output = tool
        .execute(
            serde_json::json!({
                "operation": "format",
                "input": "2026-03-08T07:30:00Z",
                "timezone": "America/New_York",
                "format_string": "%Y-%m-%d %H:%M:%S %Z"
            }),
            &ctx,
        )
        .await
        .expect("execute");

    assert_eq!(output.result["timezone"].as_str(), Some("America/New_York"));
    assert_eq!(
        output.result["formatted"].as_str(),
        Some("2026-03-08 03:30:00 EDT")
    );
}

#[tokio::test]
async fn test_invalid_timezone_returns_clear_error() {
    let tool = TimeTool;
    let ctx = JobContext::with_user("test", "chat", "test");

    let err = tool
        .execute(
            serde_json::json!({
                "operation": "now",
                "timezone": "Mars/Olympus"
            }),
            &ctx,
        )
        .await
        .expect_err("expected invalid timezone error");

    match err {
        ToolError::InvalidParameters(message) => {
            assert!(message.contains("Unknown timezone 'Mars/Olympus'"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn test_parse_naive_timestamp_with_timezone() {
    let dt = parse_timestamp("2026-03-08 03:30:00", Some(&chrono_tz::America::New_York))
        .expect("parse timestamp");

    assert_eq!(dt.to_rfc3339(), "2026-03-08T07:30:00+00:00");
}
