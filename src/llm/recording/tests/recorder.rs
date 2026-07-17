//! Tests for the recording LLM wrapper's step capture and request hints.

use super::super::*;
use crate::testing::StubLlm;

fn make_recorder(stub: Arc<StubLlm>) -> std::io::Result<RecordingLlm> {
    let dir = tempfile::tempdir()?;
    Ok(RecordingLlm::new(
        stub,
        dir.path().join("test_recording.json"),
        "test-recording".to_string(),
    ))
}

#[tokio::test]
async fn captures_user_input_before_first_response() {
    let stub = Arc::new(StubLlm::new("hello back"));
    let recorder = make_recorder(stub).expect("failed to create temp dir");

    let request = CompletionRequest::new(vec![
        ChatMessage::system("You are helpful."),
        ChatMessage::user("Hello!"),
    ]);
    recorder.complete(request).await.unwrap();

    let steps = recorder.steps.lock().await;
    assert_eq!(steps.len(), 2);

    // First step: user_input
    assert!(
        matches!(&steps[0].response, TraceResponse::UserInput { content } if content == "Hello!")
    );

    // Second step: text response
    assert!(
        matches!(&steps[1].response, TraceResponse::Text { content, .. } if content == "hello back")
    );
}

#[tokio::test]
async fn captures_text_response_correctly() {
    let stub = Arc::new(StubLlm::new("test response"));
    let recorder = make_recorder(stub).expect("failed to create temp dir");

    let request = CompletionRequest::new(vec![ChatMessage::user("question")]);
    recorder.complete(request).await.unwrap();

    let steps = recorder.steps.lock().await;
    // user_input + text
    assert_eq!(steps.len(), 2);
    match &steps[1].response {
        TraceResponse::Text {
            content,
            input_tokens,
            output_tokens,
        } => {
            assert_eq!(content, "test response");
            // StubLlm returns 0s for tokens, which is fine
            let _ = (*input_tokens, *output_tokens);
        }
        _ => panic!("Expected Text response"),
    }
}

#[tokio::test]
async fn captures_tool_calls_response() {
    let stub = Arc::new(StubLlm::new("tool result"));
    let recorder = make_recorder(stub).expect("failed to create temp dir");

    // complete_with_tools on StubLlm returns text, not tool_calls.
    // But we can still verify the recording captures it as text.
    let request = ToolCompletionRequest::new(vec![ChatMessage::user("use a tool")], vec![]);
    recorder.complete_with_tools(request).await.unwrap();

    let steps = recorder.steps.lock().await;
    assert_eq!(steps.len(), 2); // user_input + text (StubLlm doesn't return tool_calls)
}

#[tokio::test]
async fn no_spurious_user_input_for_tool_iterations() {
    let stub = Arc::new(StubLlm::new("response"));
    let recorder = make_recorder(stub).expect("failed to create temp dir");

    // First call with user message
    let request = CompletionRequest::new(vec![
        ChatMessage::system("sys"),
        ChatMessage::user("Do something"),
    ]);
    recorder.complete(request).await.unwrap();

    // Second call: same messages plus tool result (no new user message)
    let request = CompletionRequest::new(vec![
        ChatMessage::system("sys"),
        ChatMessage::user("Do something"),
        ChatMessage::assistant("I'll use a tool"),
        ChatMessage::tool_result("call_1", "echo", "result"),
    ]);
    recorder.complete(request).await.unwrap();

    let steps = recorder.steps.lock().await;
    // Step 0: user_input "Do something"
    // Step 1: text response
    // Step 2: text response (no new user_input since no new user messages)
    assert_eq!(steps.len(), 3);
    assert!(matches!(
        &steps[0].response,
        TraceResponse::UserInput { .. }
    ));
    assert!(matches!(&steps[1].response, TraceResponse::Text { .. }));
    assert!(matches!(&steps[2].response, TraceResponse::Text { .. }));
}

#[tokio::test]
async fn captures_tool_results_for_verification() {
    let stub = Arc::new(StubLlm::new("response"));
    let recorder = make_recorder(stub).expect("failed to create temp dir");

    // First call: user asks something
    let request = CompletionRequest::new(vec![
        ChatMessage::system("sys"),
        ChatMessage::user("Do something"),
    ]);
    recorder.complete(request).await.unwrap();

    // Second call: includes tool results from previous tool_calls
    let request = CompletionRequest::new(vec![
        ChatMessage::system("sys"),
        ChatMessage::user("Do something"),
        ChatMessage::assistant("I'll use a tool"),
        ChatMessage::tool_result("call_1", "echo", "echoed: hello"),
        ChatMessage::tool_result("call_2", "time", "2026-03-04T14:00:00Z"),
    ]);
    recorder.complete(request).await.unwrap();

    let steps = recorder.steps.lock().await;
    // Step 2 (the second LLM response) should have expected_tool_results
    let step = &steps[2];
    assert_eq!(step.expected_tool_results.len(), 2);
    assert_eq!(step.expected_tool_results[0].name, "echo");
    assert_eq!(step.expected_tool_results[0].content, "echoed: hello");
    assert_eq!(step.expected_tool_results[1].name, "time");
}

#[tokio::test]
async fn request_hint_extraction() {
    let stub = Arc::new(StubLlm::new("response"));
    let recorder = make_recorder(stub).expect("failed to create temp dir");

    let request = CompletionRequest::new(vec![
        ChatMessage::system("sys"),
        ChatMessage::user("What time is it?"),
    ]);
    recorder.complete(request).await.unwrap();

    let steps = recorder.steps.lock().await;
    let text_step = &steps[1];
    let hint = text_step.request_hint.as_ref().unwrap();
    assert_eq!(
        hint.last_user_message_contains.as_deref(),
        Some("What time is it?")
    );
    assert_eq!(hint.min_message_count, Some(2));
}

#[test]
fn from_env_returns_none_when_unset() {
    // SAFETY: This test is single-threaded and no other thread reads this var.
    unsafe { std::env::remove_var("AXINITE_RECORD_TRACE") };
    let stub = Arc::new(StubLlm::new("response"));
    let result = RecordingLlm::from_env(stub);
    assert!(result.is_none());
}

#[tokio::test]
async fn request_hint_handles_multibyte_utf8() {
    let stub = Arc::new(StubLlm::new("response"));
    let recorder = make_recorder(stub).expect("failed to create temp dir");

    // Create a string where byte index 80 falls inside a multi-byte char.
    // Each CJK character is 3 bytes; 26 chars × 3 bytes = 78, then "ab" = 80 bytes,
    // but let's use 27 CJK chars (81 bytes) so truncation must respect the boundary.
    let long_cjk = "你".repeat(27); // 81 bytes, > 80
    assert!(long_cjk.len() > 80);

    let request = CompletionRequest::new(vec![
        ChatMessage::system("sys"),
        ChatMessage::user(&long_cjk),
    ]);
    recorder.complete(request).await.unwrap();

    let steps = recorder.steps.lock().await;
    let text_step = &steps[1];
    let hint = text_step.request_hint.as_ref().unwrap();
    let hint_text = hint.last_user_message_contains.as_deref().unwrap();
    // Must be valid UTF-8 and not longer than 80 bytes
    assert!(hint_text.len() <= 80);
    assert!(hint_text.is_ascii() || hint_text.chars().count() > 0);
}
