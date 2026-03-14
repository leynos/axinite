//! Unit tests for the container worker runtime and its tool-advertising paths.

use std::collections::HashSet;
use std::sync::Arc;

use rstest::rstest;
use uuid::Uuid;

use super::*;

#[derive(Clone, Copy, Debug)]
enum WorkerToolSetSource {
    BuildTools,
    RuntimeDefinitions,
}

#[rstest]
#[case(WorkerToolSetSource::BuildTools, None)]
#[case(
    WorkerToolSetSource::RuntimeDefinitions,
    Some(vec![
        "apply_patch",
        "extension_info",
        "list_dir",
        "read_file",
        "shell",
        "tool_activate",
        "tool_list",
        "tool_search",
        "write_file",
    ])
)]
#[tokio::test]
async fn worker_runtime_advertises_safe_meta_tools(
    #[case] source: WorkerToolSetSource,
    #[case] expected_order: Option<Vec<&str>>,
) {
    let client = Arc::new(WorkerHttpClient::new(
        "http://localhost:50051".to_string(),
        Uuid::nil(),
        "test".to_string(),
    ));

    let names: Vec<String> = match source {
        WorkerToolSetSource::BuildTools => {
            WorkerRuntime::build_tools(Arc::clone(&client)).list().await
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
            let names: Vec<String> = runtime
                .tools
                .tool_definitions()
                .await
                .into_iter()
                .map(|def| def.name)
                .collect();
            names
        }
    };

    let expected_safe_names = [
        "apply_patch",
        "extension_info",
        "list_dir",
        "read_file",
        "shell",
        "tool_activate",
        "tool_list",
        "tool_search",
        "write_file",
    ];

    if let Some(expected_order) = expected_order {
        assert_eq!(
            names,
            expected_order
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            "worker tool order drifted for {source:?}"
        );
    } else {
        let expected_name_set: HashSet<&str> = expected_safe_names.iter().copied().collect();
        let actual_name_set: HashSet<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(
            actual_name_set, expected_name_set,
            "worker tool set drifted for {source:?}"
        );
    }

    for expected in expected_safe_names {
        assert!(
            names.iter().any(|name| name == expected),
            "expected hosted worker tool set to include {expected}, got {names:?}"
        );
    }

    for omitted in ["tool_auth", "tool_install", "tool_remove", "tool_upgrade"] {
        assert!(
            !names.iter().any(|name| name == omitted),
            "hosted worker proxy should exclude non-safe tool {omitted}, got {names:?}"
        );
    }
}
