//! Unit tests for LLM trace recording and interception.

//! Unit tests for the recording LLM wrapper.

use super::*;
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

#[tokio::test]
async fn flush_writes_valid_json_with_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.json");

    let stub = Arc::new(StubLlm::new("response"));
    let recorder = RecordingLlm::new(stub, path.clone(), "flush-test".to_string());

    // Simulate a memory snapshot
    recorder
        .memory_snapshot
        .lock()
        .await
        .push(MemorySnapshotEntry {
            path: "context/test.md".to_string(),
            content: "test content".to_string(),
        });

    // Simulate an HTTP exchange
    NativeHttpInterceptor::after_response(
        &*recorder.http_interceptor,
        &HttpExchangeRequest {
            method: "GET".to_string(),
            url: "https://api.example.com/data".to_string(),
            headers: Vec::new(),
            body: None,
        },
        &HttpExchangeResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"ok": true}"#.to_string(),
        },
    )
    .await;

    let request = CompletionRequest::new(vec![ChatMessage::user("hello")]);
    recorder.complete(request).await.unwrap();
    recorder.flush().await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let trace: TraceFile = serde_json::from_str(&content).unwrap();
    assert_eq!(trace.model_name, "flush-test");
    assert_eq!(trace.memory_snapshot.len(), 1);
    assert_eq!(trace.memory_snapshot[0].path, "context/test.md");
    assert_eq!(trace.http_exchanges.len(), 1);
    assert_eq!(trace.http_exchanges[0].response.status, 200);
    assert_eq!(trace.steps.len(), 2);
}

#[test]
fn from_env_returns_none_when_unset() {
    // SAFETY: This test is single-threaded and no other thread reads this var.
    unsafe { std::env::remove_var("IRONCLAW_RECORD_TRACE") };
    let stub = Arc::new(StubLlm::new("response"));
    let result = RecordingLlm::from_env(stub);
    assert!(result.is_none());
}

#[tokio::test]
async fn recording_http_interceptor_passes_through_and_records() {
    let interceptor = RecordingHttpInterceptor::new();

    let req = HttpExchangeRequest {
        method: "GET".to_string(),
        url: "https://example.com".to_string(),
        headers: Vec::new(),
        body: None,
    };

    // before_request should return None (pass through)
    assert!(
        NativeHttpInterceptor::before_request(&interceptor, &req)
            .await
            .is_none()
    );

    // after_response records the exchange
    let resp = HttpExchangeResponse {
        status: 200,
        headers: Vec::new(),
        body: "ok".to_string(),
    };
    NativeHttpInterceptor::after_response(&interceptor, &req, &resp).await;

    let exchanges = interceptor.take_exchanges().await;
    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].request.url, "https://example.com");
}

#[tokio::test]
async fn replaying_http_interceptor_returns_recorded_responses() {
    let exchanges = vec![HttpExchange {
        request: HttpExchangeRequest {
            method: "GET".to_string(),
            url: "https://api.example.com/data".to_string(),
            headers: Vec::new(),
            body: None,
        },
        response: HttpExchangeResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"items": []}"#.to_string(),
        },
    }];
    let interceptor = ReplayingHttpInterceptor::new(exchanges);

    // First request: returns recorded response
    let req = HttpExchangeRequest {
        method: "GET".to_string(),
        url: "https://api.example.com/data".to_string(),
        headers: Vec::new(),
        body: None,
    };
    let resp = NativeHttpInterceptor::before_request(&interceptor, &req)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, r#"{"items": []}"#);

    // Second request: no more exchanges → 599
    let resp = NativeHttpInterceptor::before_request(&interceptor, &req)
        .await
        .unwrap();
    assert_eq!(resp.status, 599);
}

#[test]
fn serde_roundtrip_extended_format() {
    let trace = TraceFile {
        model_name: "test".to_string(),
        memory_snapshot: vec![MemorySnapshotEntry {
            path: "context/vision.md".to_string(),
            content: "Be helpful.".to_string(),
        }],
        http_exchanges: vec![HttpExchange {
            request: HttpExchangeRequest {
                method: "GET".to_string(),
                url: "https://api.example.com".to_string(),
                headers: vec![("Accept".to_string(), "application/json".to_string())],
                body: None,
            },
            response: HttpExchangeResponse {
                status: 200,
                headers: Vec::new(),
                body: "{}".to_string(),
            },
        }],
        steps: vec![
            TraceStep {
                request_hint: None,
                response: TraceResponse::UserInput {
                    content: "hello".to_string(),
                },
                expected_tool_results: Vec::new(),
            },
            TraceStep {
                request_hint: Some(RequestHint {
                    last_user_message_contains: Some("hello".to_string()),
                    min_message_count: Some(2),
                }),
                response: TraceResponse::ToolCalls {
                    tool_calls: vec![TraceToolCall {
                        id: "call_1".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({"message": "hi"}),
                    }],
                    input_tokens: 50,
                    output_tokens: 20,
                },
                expected_tool_results: Vec::new(),
            },
            TraceStep {
                request_hint: None,
                response: TraceResponse::Text {
                    content: "done".to_string(),
                    input_tokens: 80,
                    output_tokens: 10,
                },
                expected_tool_results: vec![ExpectedToolResult {
                    tool_call_id: "call_1".to_string(),
                    name: "echo".to_string(),
                    content: "hi".to_string(),
                }],
            },
        ],
    };

    let json = serde_json::to_string_pretty(&trace).unwrap();
    let parsed: TraceFile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model_name, "test");
    assert_eq!(parsed.memory_snapshot.len(), 1);
    assert_eq!(parsed.http_exchanges.len(), 1);
    assert_eq!(parsed.steps.len(), 3);
    assert_eq!(parsed.steps[2].expected_tool_results.len(), 1);
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

#[test]
fn backward_compatible_with_old_format() {
    // Old format without memory_snapshot, http_exchanges, expected_tool_results
    let json = r#"{
        "model_name": "old-trace",
        "steps": [
            {
                "response": {
                    "type": "text",
                    "content": "hello",
                    "input_tokens": 10,
                    "output_tokens": 5
                }
            }
        ]
    }"#;
    let trace: TraceFile = serde_json::from_str(json).unwrap();
    assert_eq!(trace.model_name, "old-trace");
    assert!(trace.memory_snapshot.is_empty());
    assert!(trace.http_exchanges.is_empty());
    assert!(trace.steps[0].expected_tool_results.is_empty());
}
