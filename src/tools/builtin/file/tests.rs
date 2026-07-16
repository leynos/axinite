//! Unit tests for the file read and write tools.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::context::JobContext;
use crate::tools::builtin::path_utils::{normalize_lexical, validate_path};
use crate::tools::tool::NativeTool;

use super::{ApplyPatchTool, ListDirTool, ReadFileTool, WriteFileTool};

#[tokio::test]
async fn test_read_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.txt");
    ambient_fs::write(&file_path, "line 1\nline 2\nline 3\n").unwrap();

    let tool = ReadFileTool::new().with_base_dir(dir.path().to_path_buf());
    let ctx = JobContext::default();

    let result = tool
        .execute(
            serde_json::json!({"path": file_path.to_str().unwrap()}),
            &ctx,
        )
        .await
        .unwrap();

    let content = result.result.get("content").unwrap().as_str().unwrap();
    assert!(content.contains("line 1"));
    assert!(content.contains("line 2"));
}

#[tokio::test]
async fn test_write_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("new_file.txt");

    let tool = WriteFileTool::new().with_base_dir(dir.path().to_path_buf());
    let ctx = JobContext::default();

    let result = tool
        .execute(
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "content": "hello world"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.result.get("success").unwrap().as_bool().unwrap());
    assert_eq!(
        ambient_fs::read_to_string(&file_path).unwrap(),
        "hello world"
    );
}

#[tokio::test]
async fn test_apply_patch() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("code.rs");
    ambient_fs::write(&file_path, "fn main() {\n    println!(\"old\");\n}\n").unwrap();

    let tool = ApplyPatchTool::new().with_base_dir(dir.path().to_path_buf());
    let ctx = JobContext::default();

    let result = tool
        .execute(
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "old_string": "println!(\"old\")",
                "new_string": "println!(\"new\")"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.result.get("success").unwrap().as_bool().unwrap());
    let content = ambient_fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("println!(\"new\")"));
}

#[tokio::test]
async fn test_write_file_rejects_workspace_paths() {
    let dir = TempDir::new().unwrap();
    let tool = WriteFileTool::new().with_base_dir(dir.path().to_path_buf());
    let ctx = JobContext::default();

    let workspace_files = &[
        "HEARTBEAT.md",
        "MEMORY.md",
        "IDENTITY.md",
        "SOUL.md",
        "AGENTS.md",
        "USER.md",
        "README.md",
    ];

    for filename in workspace_files {
        let path = dir.path().join(filename);
        let err = tool
            .execute(
                serde_json::json!({
                    "path": path.to_str().unwrap(),
                    "content": "test"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("memory_write"),
            "Rejection for {} should mention memory_write, got: {}",
            filename,
            msg
        );
    }

    // daily/ and context/ prefixes should also be rejected
    for prefix_path in &["daily/2024-01-15.md", "context/vision.md"] {
        let err = tool
            .execute(
                serde_json::json!({
                    "path": prefix_path,
                    "content": "test"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        assert!(
            err.to_string().contains("memory_write"),
            "Rejection for {} should mention memory_write",
            prefix_path
        );
    }

    // Regular files should still work
    let regular_path = dir.path().join("normal.txt");
    let result = tool
        .execute(
            serde_json::json!({
                "path": regular_path.to_str().unwrap(),
                "content": "fine"
            }),
            &ctx,
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_dir() {
    let dir = TempDir::new().unwrap();
    ambient_fs::write(dir.path().join("file1.txt"), "content").unwrap();
    ambient_fs::create_dir(dir.path().join("subdir")).unwrap();

    let tool = ListDirTool::new();
    let ctx = JobContext::default();

    let result = tool
        .execute(
            serde_json::json!({"path": dir.path().to_str().unwrap()}),
            &ctx,
        )
        .await
        .unwrap();

    let entries = result.result.get("entries").unwrap().as_array().unwrap();
    assert!(entries.len() >= 2);
}

#[test]
fn test_normalize_lexical() {
    // Basic .. resolution
    assert_eq!(
        normalize_lexical(Path::new("/a/b/../c")),
        PathBuf::from("/a/c")
    );
    // Multiple .. components
    assert_eq!(
        normalize_lexical(Path::new("/a/b/c/../../d")),
        PathBuf::from("/a/d")
    );
    // . components stripped
    assert_eq!(
        normalize_lexical(Path::new("/a/./b/./c")),
        PathBuf::from("/a/b/c")
    );
    // Cannot escape root
    assert_eq!(
        normalize_lexical(Path::new("/a/../../..")),
        PathBuf::from("/")
    );
}

#[test]
fn test_validate_path_rejects_traversal_nonexistent_parent() {
    // The critical test: writing to ../../outside/newdir/file with base_dir
    // set should be rejected even when the parent directory does not exist
    // (i.e. canonicalize() cannot resolve it).
    let dir = TempDir::new().unwrap();
    let evil_path = format!(
        "{}/../../outside/newdir/file.txt",
        dir.path().to_str().unwrap()
    );
    let result = validate_path(&evil_path, Some(dir.path()));
    assert!(
        result.is_err(),
        "Should reject traversal via non-existent parent, got: {:?}",
        result
    );
}

#[test]
fn test_validate_path_rejects_relative_traversal() {
    let dir = TempDir::new().unwrap();
    let result = validate_path("../../etc/passwd", Some(dir.path()));
    assert!(
        result.is_err(),
        "Should reject relative traversal, got: {:?}",
        result
    );
}

#[test]
fn test_validate_path_allows_valid_nested_write() {
    let dir = TempDir::new().unwrap();
    let result = validate_path("subdir/newfile.txt", Some(dir.path()));
    assert!(
        result.is_ok(),
        "Should allow nested writes within sandbox: {:?}",
        result
    );
}

#[test]
fn test_validate_path_allows_dot_dot_within_sandbox() {
    // a/b/../c resolves to a/c which is still inside the sandbox
    let dir = TempDir::new().unwrap();
    ambient_fs::create_dir_all(dir.path().join("a/b")).unwrap();
    let result = validate_path("a/b/../c.txt", Some(dir.path()));
    assert!(
        result.is_ok(),
        "Should allow .. that stays within sandbox: {:?}",
        result
    );
}
