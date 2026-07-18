//! Tests for attachment path validation, sandbox containment, and
//! attachment delivery to channel broadcast.

use std::sync::Arc;

use crate::channels::ChannelManager;
use crate::tools::builtin::message::MessageTool;
use crate::tools::tool::NativeTool;

#[tokio::test]
async fn message_tool_with_attachments_outside_sandbox() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));

    // Set context
    tool.set_context(Some("signal".to_string()), Some("+1234567890".to_string()))
        .await;

    // Execute with attachments outside both sandbox (~/.axinite) and /tmp/
    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "content": "hello",
                "attachments": ["/etc/passwd", "/var/log/syslog"]
            }),
            &ctx,
        )
        .await;

    // Should fail due to sandbox rejection (paths outside allowed directories)
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("sandbox") || err.contains("escapes") || err.contains("must be within"),);
}

#[tokio::test]
async fn message_tool_with_attachments_inside_sandbox_no_channel() {
    use ambient_fs as fs;

    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    tool.set_context(Some("signal".to_string()), Some("+1234567890".to_string()))
        .await;

    // Create temp files inside the sandbox
    let sandbox_dir = &tool.base_dir;
    let temp_dir = tempfile::tempdir_in(sandbox_dir).unwrap();
    let file1 = temp_dir.path().join("file1.txt");
    let file2 = temp_dir.path().join("file2.png");
    fs::write(&file1, "test").unwrap();
    fs::write(&file2, "test").unwrap();

    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "content": "hello",
                "attachments": [file1.to_string_lossy(), file2.to_string_lossy()]
            }),
            &ctx,
        )
        .await;

    // Path validation passes, but channel broadcast fails (no real channel)
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("channel") || err.contains("Channel"));
}

#[tokio::test]
async fn message_tool_with_attachments_in_tmp_no_channel() {
    use ambient_fs as fs;

    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    tool.set_context(Some("telegram".to_string()), Some("12345".to_string()))
        .await;

    // Create temp files under /tmp (allowed as secondary attachment dir)
    let temp_dir = tempfile::tempdir_in("/tmp").unwrap();
    let file1 = temp_dir.path().join("photo.jpg");
    let file2 = temp_dir.path().join("doc.pdf");
    fs::write(&file1, "fake image data").unwrap();
    fs::write(&file2, "fake pdf data").unwrap();

    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "content": "here are the files",
                "attachments": [file1.to_string_lossy(), file2.to_string_lossy()]
            }),
            &ctx,
        )
        .await;

    // Path validation passes for /tmp paths, fails at channel send (no real channel)
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("channel") || err.contains("Channel"),
        "expected channel error (path validation should pass), got: {}",
        err
    );
}

#[test]
fn path_traversal_rejects_double_dot() {
    use crate::tools::builtin::path_utils::is_path_safe_basic;
    assert!(!is_path_safe_basic("../etc/passwd"));
    assert!(!is_path_safe_basic("foo/../bar"));
    assert!(!is_path_safe_basic("foo/bar/../../secret"));
}

#[test]
fn path_traversal_accepts_normal_paths() {
    use crate::tools::builtin::path_utils::is_path_safe_basic;
    assert!(is_path_safe_basic("/tmp/file.txt"));
    assert!(is_path_safe_basic("documents/report.pdf"));
    assert!(is_path_safe_basic("my-file.png"));
}

#[tokio::test]
async fn message_tool_rejects_path_traversal_attachments() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    tool.set_context(Some("signal".to_string()), Some("+1234567890".to_string()))
        .await;

    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "content": "here's the file",
                "attachments": ["../../../etc/passwd"]
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("forbidden") || err.contains(".."));
}

#[tokio::test]
async fn message_tool_passes_attachment_to_broadcast() {
    use ambient_fs as fs;

    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    tool.set_context(Some("signal".to_string()), Some("+1234567890".to_string()))
        .await;

    // Create a temp file within the sandbox directory
    let sandbox_dir = &tool.base_dir;
    let temp_dir = tempfile::tempdir_in(sandbox_dir).unwrap();
    let temp_path = temp_dir.path().join("test.txt");
    fs::write(&temp_path, "test content").unwrap();
    let temp_path_str = temp_path.to_string_lossy().to_string();

    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "content": "here's the file",
                "attachments": [temp_path_str]
            }),
            &ctx,
        )
        .await;

    // Should succeed in path validation (file is in sandbox)
    // but fail on channel broadcast (no actual channel)
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found") || err.contains("Failed") || err.contains("broadcast"),
        "Expected channel error, got: {}",
        err
    );
}

#[tokio::test]
async fn message_tool_passes_multiple_attachments_to_broadcast() {
    use ambient_fs as fs;

    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    tool.set_context(Some("signal".to_string()), Some("+1234567890".to_string()))
        .await;

    // Create temp files within the sandbox directory
    let sandbox_dir = &tool.base_dir;
    let temp_dir = tempfile::tempdir_in(sandbox_dir).unwrap();
    let temp_path1 = temp_dir.path().join("test1.txt");
    let temp_path2 = temp_dir.path().join("test2.txt");
    fs::write(&temp_path1, "test content 1").unwrap();
    fs::write(&temp_path2, "test content 2").unwrap();
    let path1 = temp_path1.to_string_lossy().to_string();
    let path2 = temp_path2.to_string_lossy().to_string();

    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "content": "files attached",
                "attachments": [path1, path2]
            }),
            &ctx,
        )
        .await;

    // Should succeed in path validation (files are in sandbox)
    // but fail on channel broadcast (no actual channel)
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found") || err.contains("Failed") || err.contains("broadcast"),
        "Expected channel error, got: {}",
        err
    );
}
