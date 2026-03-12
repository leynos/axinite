use std::sync::Arc;

use rstest::rstest;
use uuid::Uuid;

use super::*;
use crate::agent::agentic_loop::truncate_for_preview;

#[test]
fn test_truncate_within_limit() {
    assert_eq!(truncate_for_preview("hello", 10), "hello");
}

#[test]
fn test_truncate_at_limit() {
    assert_eq!(truncate_for_preview("hello", 5), "hello");
}

#[test]
fn test_truncate_beyond_limit() {
    let result = truncate_for_preview("hello world", 5);
    assert_eq!(result, "hello...");
}

#[test]
fn test_truncate_multibyte_safe() {
    // "é" is 2 bytes in UTF-8; slicing at byte 1 would panic without safety
    let result = truncate_for_preview("é is fancy", 1);
    // Should truncate to 0 chars (can't fit "é" in 1 byte)
    assert_eq!(result, "...");
}

#[derive(Clone, Copy)]
enum WorkerToolSetSource {
    BuildTools,
    RuntimeDefinitions,
}

#[rstest]
#[case(WorkerToolSetSource::BuildTools)]
#[case(WorkerToolSetSource::RuntimeDefinitions)]
#[tokio::test]
async fn worker_runtime_advertises_safe_meta_tools(#[case] source: WorkerToolSetSource) {
    let client = Arc::new(WorkerHttpClient::new(
        "http://localhost:50051".to_string(),
        Uuid::nil(),
        "test".to_string(),
    ));

    let names: Vec<String> = match source {
        WorkerToolSetSource::BuildTools => {
            let tools = WorkerRuntime::build_tools(Arc::clone(&client));
            let mut names = tools.list().await;
            names.sort();
            names
        }
        WorkerToolSetSource::RuntimeDefinitions => {
            let runtime = WorkerRuntime::from_client(
                WorkerConfig {
                    job_id: Uuid::nil(),
                    orchestrator_url: "http://localhost:50051".to_string(),
                    ..WorkerConfig::default()
                },
                client,
            );
            let mut names: Vec<String> = runtime
                .tools
                .tool_definitions()
                .await
                .into_iter()
                .map(|def| def.name)
                .collect();
            names.sort();
            names
        }
    };

    for expected in [
        "apply_patch",
        "extension_info",
        "list_dir",
        "read_file",
        "shell",
        "tool_list",
        "tool_search",
        "write_file",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "expected hosted worker tool set to include {expected}, got {names:?}"
        );
    }

    for omitted in [
        "tool_activate",
        "tool_auth",
        "tool_install",
        "tool_remove",
        "tool_upgrade",
    ] {
        assert!(
            !names.iter().any(|name| name == omitted),
            "hosted worker proxy should exclude approval-gated tool {omitted}, got {names:?}"
        );
    }
}
