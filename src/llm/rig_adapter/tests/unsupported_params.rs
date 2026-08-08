//! Tests for unsupported-parameter stripping in provider-backed rig adapters.

use super::*;
use rig::completion::CompletionModel;
use rstest::fixture;

/// Build a rig adapter backed by a throwaway OpenAI client.
///
/// Constructing the client is arrangement and can fail, so the fixture yields
/// a [`Result`] that each test body unwraps.
#[fixture]
fn openai_rig_adapter() -> anyhow::Result<RigAdapter<impl CompletionModel>> {
    use rig::client::CompletionClient;
    use rig::providers::openai;

    let client: openai::Client = openai::Client::builder()
        .api_key("test-key")
        .base_url("http://localhost:0")
        .build()
        .map_err(|error| anyhow::anyhow!("failed to build test client: {error}"))?;
    let client = client.completions_api();
    let model = client.completion_model("test-model");
    Ok(RigAdapter::new(model, "test-model"))
}

#[rstest]
fn test_with_unsupported_params_populates_set(
    openai_rig_adapter: anyhow::Result<RigAdapter<impl CompletionModel>>,
) {
    let adapter = openai_rig_adapter
        .expect("build test rig adapter")
        .with_unsupported_params(vec!["temperature".to_string()]);

    assert!(adapter.unsupported_params.contains("temperature"));
    assert!(!adapter.unsupported_params.contains("max_tokens"));
}

#[rstest]
fn test_strip_unsupported_completion_params(
    openai_rig_adapter: anyhow::Result<RigAdapter<impl CompletionModel>>,
) {
    let adapter = openai_rig_adapter
        .expect("build test rig adapter")
        .with_unsupported_params(vec![
            "temperature".to_string(),
            "stop_sequences".to_string(),
        ]);

    let mut req = CompletionRequest::new(vec![ChatMessage::user("hi")]);
    req.temperature = Some(0.7);
    req.max_tokens = Some(100);
    req.stop_sequences = Some(vec!["STOP".to_string()]);

    adapter.strip_unsupported_completion_params(&mut req);

    assert!(req.temperature.is_none(), "temperature should be stripped");
    assert_eq!(req.max_tokens, Some(100), "max_tokens should be preserved");
    assert!(
        req.stop_sequences.is_none(),
        "stop_sequences should be stripped"
    );
}

#[rstest]
fn test_strip_unsupported_tool_params(
    openai_rig_adapter: anyhow::Result<RigAdapter<impl CompletionModel>>,
) {
    let adapter = openai_rig_adapter
        .expect("build test rig adapter")
        .with_unsupported_params(vec!["temperature".to_string(), "max_tokens".to_string()]);

    let mut req = ToolCompletionRequest::new(vec![ChatMessage::user("hi")], vec![]);
    req.temperature = Some(0.5);
    req.max_tokens = Some(200);

    adapter.strip_unsupported_tool_params(&mut req);

    assert!(req.temperature.is_none(), "temperature should be stripped");
    assert!(req.max_tokens.is_none(), "max_tokens should be stripped");
}

#[rstest]
fn test_unsupported_params_empty_by_default(
    openai_rig_adapter: anyhow::Result<RigAdapter<impl CompletionModel>>,
) {
    let adapter = openai_rig_adapter.expect("build test rig adapter");

    assert!(adapter.unsupported_params.is_empty());
}
