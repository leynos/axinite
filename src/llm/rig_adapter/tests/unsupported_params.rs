//! Tests for unsupported-parameter stripping in provider-backed rig adapters.

use super::*;
use rig::completion::CompletionModel;
use rstest::fixture;

#[fixture]
fn openai_rig_adapter() -> RigAdapter<impl CompletionModel> {
    use rig::client::CompletionClient;
    use rig::providers::openai;

    let client: openai::Client = openai::Client::builder()
        .api_key("test-key")
        .base_url("http://localhost:0")
        .build()
        .expect("failed to build test client");
    let client = client.completions_api();
    let model = client.completion_model("test-model");
    RigAdapter::new(model, "test-model")
}

#[rstest]
fn test_with_unsupported_params_populates_set(
    openai_rig_adapter: RigAdapter<impl CompletionModel>,
) {
    let adapter = openai_rig_adapter.with_unsupported_params(vec!["temperature".to_string()]);

    assert!(adapter.unsupported_params.contains("temperature"));
    assert!(!adapter.unsupported_params.contains("max_tokens"));
}

#[rstest]
fn test_strip_unsupported_completion_params(openai_rig_adapter: RigAdapter<impl CompletionModel>) {
    let adapter = openai_rig_adapter.with_unsupported_params(vec![
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
fn test_strip_unsupported_tool_params(openai_rig_adapter: RigAdapter<impl CompletionModel>) {
    let adapter = openai_rig_adapter
        .with_unsupported_params(vec!["temperature".to_string(), "max_tokens".to_string()]);

    let mut req = ToolCompletionRequest::new(vec![ChatMessage::user("hi")], vec![]);
    req.temperature = Some(0.5);
    req.max_tokens = Some(200);

    adapter.strip_unsupported_tool_params(&mut req);

    assert!(req.temperature.is_none(), "temperature should be stripped");
    assert!(req.max_tokens.is_none(), "max_tokens should be stripped");
}

#[rstest]
fn test_unsupported_params_empty_by_default(openai_rig_adapter: RigAdapter<impl CompletionModel>) {
    assert!(openai_rig_adapter.unsupported_params.is_empty());
}
