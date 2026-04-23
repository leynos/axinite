//! Trace-type helper tests.

use crate::support::trace_test_files::write_tmp_trace;
use crate::support::trace_types::load_trace_with_mutation;

#[tokio::test]
async fn mutation_is_applied() -> anyhow::Result<()> {
    let json = serde_json::json!({"model_name": "test-model", "steps": []}).to_string();
    let tmp = write_tmp_trace(&json)?;
    let mut called = false;

    let trace = load_trace_with_mutation(tmp.path(), |_value| {
        called = true;
    })
    .await?;

    assert!(called, "mutation closure must be invoked");
    assert_eq!(trace.model_name, "test-model");
    assert!(trace.steps.is_empty());
    Ok(())
}

#[tokio::test]
async fn mutation_modifies_value() -> anyhow::Result<()> {
    let json = serde_json::json!({"model_name": "original", "steps": []}).to_string();
    let tmp = write_tmp_trace(&json)?;

    let trace = load_trace_with_mutation(tmp.path(), |value| {
        value["model_name"] = serde_json::json!("mutated");
    })
    .await?;

    assert_eq!(trace.model_name, "mutated");
    Ok(())
}

#[tokio::test]
async fn missing_file_returns_error() {
    let result = load_trace_with_mutation("/nonexistent/path/trace.json", |_value| {}).await;
    assert!(result.is_err(), "missing file must return Err");
}

#[tokio::test]
async fn invalid_json_returns_error() -> anyhow::Result<()> {
    let tmp = write_tmp_trace("not valid json {{")?;
    let result = load_trace_with_mutation(tmp.path(), |_value| {}).await;
    assert!(result.is_err(), "invalid JSON must return Err");
    Ok(())
}
