//! Trace-type helper tests.

use crate::support::trace_test_files::write_tmp_trace;
use crate::support::trace_types::load_trace_with_mutation;

use rstest::rstest;

#[derive(Clone, Copy)]
enum MutationBehavior {
    SetsFlag,
    MutatesModelName,
}

#[rstest]
#[case::sets_flag(
    r#"{"model_name":"test-model","steps":[]}"#,
    MutationBehavior::SetsFlag
)]
#[case::mutates_model_name(
    r#"{"model_name":"original","steps":[]}"#,
    MutationBehavior::MutatesModelName
)]
#[tokio::test]
async fn mutation_happy_paths(
    #[case] initial_json: &str,
    #[case] behavior: MutationBehavior,
) -> anyhow::Result<()> {
    let tmp = write_tmp_trace(initial_json)?;
    let mut was_called = false;

    let trace = load_trace_with_mutation(tmp.path(), |value| match behavior {
        MutationBehavior::SetsFlag => was_called = true,
        MutationBehavior::MutatesModelName => value["model_name"] = serde_json::json!("mutated"),
    })
    .await?;

    match behavior {
        MutationBehavior::SetsFlag => {
            assert!(was_called, "mutation closure must be invoked");
            assert_eq!(trace.model_name, "test-model");
            assert!(trace.steps.is_empty());
        }
        MutationBehavior::MutatesModelName => {
            assert_eq!(trace.model_name, "mutated");
        }
    }

    Ok(())
}

#[tokio::test]
async fn missing_file_returns_error() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let missing_trace = temp_dir.path().join("trace.json");

    let result = load_trace_with_mutation(&missing_trace, |_value| {}).await;
    assert!(result.is_err(), "missing file must return Err");
    Ok(())
}

#[tokio::test]
async fn invalid_json_returns_error() -> anyhow::Result<()> {
    let tmp = write_tmp_trace("not valid json {{")?;
    let result = load_trace_with_mutation(tmp.path(), |_value| {}).await;
    assert!(result.is_err(), "invalid JSON must return Err");
    Ok(())
}
