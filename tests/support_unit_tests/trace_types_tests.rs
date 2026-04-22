//! Trace-type helper tests.

use std::io::Write;

use crate::support::trace_types::load_trace_with_mutation;
use tempfile::NamedTempFile;

fn write_tmp_trace(json: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temporary trace file");
    file.write_all(json.as_bytes())
        .expect("write temporary trace JSON");
    file
}

#[tokio::test]
async fn mutation_is_applied() {
    let json = serde_json::json!({"model_name": "test-model", "steps": []}).to_string();
    let tmp = write_tmp_trace(&json);
    let mut called = false;

    let trace = load_trace_with_mutation(tmp.path(), |_value| {
        called = true;
    })
    .await
    .expect("should deserialize");

    assert!(called, "mutation closure must be invoked");
    assert_eq!(trace.model_name, "test-model");
    assert!(trace.steps.is_empty());
}

#[tokio::test]
async fn mutation_modifies_value() {
    let json = serde_json::json!({"model_name": "original", "steps": []}).to_string();
    let tmp = write_tmp_trace(&json);

    let trace = load_trace_with_mutation(tmp.path(), |value| {
        value["model_name"] = serde_json::json!("mutated");
    })
    .await
    .expect("should deserialize");

    assert_eq!(trace.model_name, "mutated");
}

#[tokio::test]
async fn missing_file_returns_error() {
    let result = load_trace_with_mutation("/nonexistent/path/trace.json", |_value| {}).await;
    assert!(result.is_err(), "missing file must return Err");
}

#[tokio::test]
async fn invalid_json_returns_error() {
    let tmp = write_tmp_trace("not valid json {{");
    let result = load_trace_with_mutation(tmp.path(), |_value| {}).await;
    assert!(result.is_err(), "invalid JSON must return Err");
}
