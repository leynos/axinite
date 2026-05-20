//! Tests for REPL status output rendering.

use insta::assert_snapshot;

use super::*;

#[test]
fn status_output_stdout_snapshot() {
    let stdout = ["stream-one", "stream-two"].join("");
    let separator = render_stream_chunk_separator(80);

    assert_snapshot!(&stdout, @"stream-onestream-two");
    assert_snapshot!(&separator);
}

fn tool_output_section() -> String {
    [
        render_thinking("Thinking about the next step"),
        render_tool_started("write_file"),
        render_tool_completed_lines(&ToolCompletedInfo {
            name: "write_file",
            success: true,
            error: None,
            parameters: None,
        })
        .join("\n"),
        render_tool_completed_lines(&ToolCompletedInfo {
            name: "write_file",
            success: false,
            error: Some("permission denied"),
            parameters: Some("{\"path\":\"[REDACTED]\",\"mode\":\"0644\"}"),
        })
        .join("\n"),
        render_tool_result("Wrote file successfully"),
    ]
    .join("\n")
}

fn job_status_section() -> String {
    [
        render_job_started(&JobStartedInfo {
            job_id: "job_123",
            title: "Inspect docs",
            browse_url: "https://example.test/job/123",
        }),
        render_status(true, "status for debug").expect("debug status should render"),
        render_status(false, "Approval required for write_file")
            .expect("approval status should render"),
    ]
    .join("\n")
}

fn approval_section() -> String {
    let request = ToolApprovalRequest {
        request_id: "req_123456789",
        tool_name: "write_file",
        description: "Write a file after approval",
    };
    render_approval_needed_lines(
        &request,
        &serde_json::json!({"path": "/tmp/example.txt", "mode": "0644"}),
    )
    .join("\n")
}

fn auth_image_section() -> String {
    [
        render_auth_required_lines(&AuthRequiredInfo {
            extension_name: "github",
            instructions: Some("Visit the OAuth URL to continue."),
            setup_url: Some("https://example.test/setup"),
            auth_url: Some("https://example.test/auth"),
        })
        .join("\n"),
        render_auth_completed(&AuthCompletedInfo {
            extension_name: "github",
            success: true,
            message: "Authenticated",
        }),
        render_auth_completed(&AuthCompletedInfo {
            extension_name: "github",
            success: false,
            message: "Authentication failed",
        }),
        render_image_generated(Some("/tmp/output.png")),
        render_image_generated(None),
    ]
    .join("\n")
}

#[test]
fn status_output_stderr_snapshot() {
    let stderr = [
        tool_output_section(),
        job_status_section(),
        approval_section(),
        auth_image_section(),
    ]
    .join("\n");

    assert_snapshot!(&stderr);
}
