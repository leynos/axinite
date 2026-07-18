//! Chat completion behaviour tests: basic completions, system messages,
//! tool calls, streaming, and model override handling.

use std::sync::Arc;

use axinite::llm::LlmProvider;

use super::helpers::{
    AUTH_TOKEN, FixedModelProvider, client, start_test_server, start_test_server_with_provider,
};

#[tokio::test]
async fn test_chat_completions_basic() {
    let (addr, _state, mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": [
                {"role": "user", "content": "Hello world"}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["model"], "mock-model-v1");
    assert_eq!(body["choices"][0]["finish_reason"], "stop");

    let content = body["choices"][0]["message"]["content"].as_str().unwrap();
    assert!(
        content.contains("Hello world"),
        "Expected echo, got: {}",
        content
    );

    // Check usage
    assert_eq!(body["usage"]["prompt_tokens"], 10);
    assert_eq!(body["usage"]["completion_tokens"], 5);
    assert_eq!(body["usage"]["total_tokens"], 15);

    let models = mock_state.completion_models.lock().await;
    assert_eq!(*models, vec![Some("mock-model-v1".to_string())]);
}

#[tokio::test]
async fn test_chat_completions_with_system_message() {
    let (addr, _state, _mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "What is 2+2?"}
            ],
            "temperature": 0.5,
            "max_tokens": 100
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let content = body["choices"][0]["message"]["content"].as_str().unwrap();
    assert!(content.contains("2+2"));
}

#[tokio::test]
async fn test_chat_completions_with_tools() {
    let (addr, _state, mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": [
                {"role": "user", "content": "What's the weather?"}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get the weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        }
                    }
                }
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");

    let tool_calls = &body["choices"][0]["message"]["tool_calls"];
    assert!(tool_calls.is_array());
    assert_eq!(tool_calls[0]["id"], "call_mock_001");
    assert_eq!(tool_calls[0]["type"], "function");
    assert_eq!(tool_calls[0]["function"]["name"], "get_weather");

    let models = mock_state.tool_completion_models.lock().await;
    assert_eq!(*models, vec![Some("mock-model-v1".to_string())]);
}

#[tokio::test]
async fn test_chat_completions_streaming() {
    let (addr, _state, mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": [
                {"role": "user", "content": "Stream test"}
            ],
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Check simulated streaming header
    assert_eq!(
        resp.headers()
            .get("x-axinite-streaming")
            .and_then(|v| v.to_str().ok()),
        Some("simulated"),
        "Expected x-axinite-streaming: simulated header"
    );

    let text = resp.text().await.unwrap();

    // Should contain SSE data lines
    assert!(
        text.contains("data:"),
        "Expected SSE data lines, got: {}",
        text
    );
    // Should end with [DONE]
    assert!(
        text.contains("[DONE]"),
        "Expected [DONE] sentinel, got: {}",
        text
    );
    // Should contain the role chunk
    assert!(
        text.contains("\"role\":\"assistant\""),
        "Expected role chunk, got: {}",
        text
    );

    // Collect all content from the chunks
    let mut full_content = String::new();
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                continue;
            }
            if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data)
                && let Some(content) = chunk["choices"][0]["delta"]["content"].as_str()
            {
                full_content.push_str(content);
            }
        }
    }
    assert!(
        full_content.contains("Stream test"),
        "Expected reassembled content to contain 'Stream test', got: '{}'",
        full_content
    );

    let models = mock_state.completion_models.lock().await;
    assert_eq!(*models, vec![Some("mock-model-v1".to_string())]);
}

#[tokio::test]
async fn test_chat_completions_empty_messages() {
    let (addr, _state, _mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": []
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]["message"].as_str().unwrap().contains("empty"));
}

#[tokio::test]
async fn test_chat_completions_model_override() {
    let (addr, _state, mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["model"], "gpt-4");

    let models = mock_state.completion_models.lock().await;
    assert_eq!(*models, vec![Some("gpt-4".to_string())]);
}

#[tokio::test]
async fn test_chat_completions_uses_effective_model_when_override_ignored() {
    let provider: Arc<dyn LlmProvider> = Arc::new(FixedModelProvider::new("configured-model"));
    let (addr, _state) = start_test_server_with_provider(provider).await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["model"], "configured-model");
}

#[tokio::test]
async fn test_chat_completions_streaming_uses_effective_model_when_override_ignored() {
    let provider: Arc<dyn LlmProvider> = Arc::new(FixedModelProvider::new("configured-model"));
    let (addr, _state) = start_test_server_with_provider(provider).await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("\"model\":\"configured-model\""),
        "Expected streaming chunks to report configured model, got: {}",
        text
    );
}
