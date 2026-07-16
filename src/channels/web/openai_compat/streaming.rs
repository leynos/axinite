//! Simulated SSE streaming for `/v1/chat/completions` with `stream: true`.

use std::sync::Arc;

use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};

use crate::llm::{ChatMessage, FinishReason};

use super::convert::{
    build_completion_request, build_tool_request, chat_completion_id, convert_messages,
    finish_reason_str, map_llm_error, openai_error, unix_timestamp,
};
use super::types::{
    OpenAiChatChunk, OpenAiChatRequest, OpenAiChunkChoice, OpenAiDelta, OpenAiErrorResponse,
    OpenAiToolCallDelta, OpenAiToolCallFunctionDelta,
};

/// Completed LLM output before it is re-chunked for simulated streaming.
enum LlmResult {
    Simple(crate::llm::CompletionResponse),
    WithTools(crate::llm::ToolCompletionResponse),
}

/// Execute the LLM call (with or without tools) before streaming starts.
async fn execute_llm(
    llm: &Arc<dyn crate::llm::LlmProvider>,
    req: &OpenAiChatRequest,
    messages: Vec<ChatMessage>,
    has_tools: bool,
) -> Result<LlmResult, (StatusCode, Json<OpenAiErrorResponse>)> {
    if has_tools {
        let tool_req = build_tool_request(req, messages);
        Ok(LlmResult::WithTools(
            llm.complete_with_tools(tool_req)
                .await
                .map_err(map_llm_error)?,
        ))
    } else {
        let comp_req = build_completion_request(req, messages);
        Ok(LlmResult::Simple(
            llm.complete(comp_req).await.map_err(map_llm_error)?,
        ))
    }
}

/// Handle streaming responses.
///
/// The current `LlmProvider` returns complete responses (no streaming method).
/// We execute the LLM call first, then simulate chunked delivery by splitting
/// the response into word-boundary chunks. This ensures LLM failures return
/// proper HTTP errors instead of SSE error events. True token streaming can be
/// added later by extending `LlmProvider` with a `complete_stream()` method.
pub(super) async fn handle_streaming(
    llm: Arc<dyn crate::llm::LlmProvider>,
    req: OpenAiChatRequest,
    has_tools: bool,
) -> Result<Response, (StatusCode, Json<OpenAiErrorResponse>)> {
    let messages = convert_messages(&req.messages)
        .map_err(|e| openai_error(StatusCode::BAD_REQUEST, e, "invalid_request_error"))?;

    let requested_model = req.model.clone();

    // Execute the LLM call before starting the SSE stream.
    // Since streaming is simulated (LlmProvider returns complete responses),
    // this lets us return proper HTTP errors on failure.
    let llm_result = execute_llm(&llm, &req, messages, has_tools).await?;
    let model_name = llm.effective_model_name(Some(requested_model.as_str()));

    // LLM succeeded — emit the response as SSE chunks
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, std::convert::Infallible>>(64);

    let emitter = ChunkEmitter {
        tx,
        id: chat_completion_id(),
        created: unix_timestamp(),
        model: model_name,
    };
    tokio::spawn(emitter.emit(llm_result));

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let sse = Sse::new(stream).keep_alive(KeepAlive::new().text(""));
    let mut response = sse.into_response();
    response.headers_mut().insert(
        "x-ironclaw-streaming",
        HeaderValue::from_static("simulated"),
    );
    Ok(response)
}

/// Emits one simulated SSE chunk stream for a completed LLM response.
struct ChunkEmitter {
    tx: tokio::sync::mpsc::Sender<Result<Event, std::convert::Infallible>>,
    id: String,
    created: u64,
    model: String,
}

impl ChunkEmitter {
    /// Wrap a delta (and optional finish reason) in a chunk envelope.
    fn chunk(&self, delta: OpenAiDelta, finish_reason: Option<String>) -> OpenAiChatChunk {
        OpenAiChatChunk {
            id: self.id.clone(),
            object: "chat.completion.chunk",
            created: self.created,
            model: self.model.clone(),
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta,
                finish_reason,
            }],
        }
    }

    /// Serialize and send one chunk; returns `false` when the receiver hung up.
    async fn send_chunk(&self, chunk: OpenAiChatChunk) -> bool {
        let data = serde_json::to_string(&chunk).unwrap_or_default();
        self.tx.send(Ok(Event::default().data(data))).await.is_ok()
    }

    /// Emit the full stream: role, content/tool-call deltas, finish reason,
    /// and the `[DONE]` sentinel.
    async fn emit(self, llm_result: LlmResult) {
        // Send initial chunk with role
        let role_chunk = self.chunk(OpenAiDelta::with_role("assistant"), None);
        let _ = self.send_chunk(role_chunk).await;

        match llm_result {
            LlmResult::WithTools(resp) => {
                // Stream content chunks
                if let Some(ref content) = resp.content {
                    self.stream_content_chunks(content).await;
                }

                // Stream tool calls
                if !resp.tool_calls.is_empty() {
                    self.send_tool_call_chunk(&resp.tool_calls).await;
                }

                // Final chunk with finish_reason
                self.send_finish_chunk(resp.finish_reason).await;
            }
            LlmResult::Simple(resp) => {
                self.stream_content_chunks(&resp.content).await;
                self.send_finish_chunk(resp.finish_reason).await;
            }
        }

        // Send [DONE] sentinel
        let _ = self.tx.send(Ok(Event::default().data("[DONE]"))).await;
    }

    /// Split content into word-boundary chunks (~20 chars) and send them.
    async fn stream_content_chunks(&self, content: &str) {
        let mut buf = String::new();
        for word in content.split_inclusive(char::is_whitespace) {
            buf.push_str(word);
            if buf.len() >= 20 {
                if !self.send_content_chunk(buf.clone()).await {
                    return;
                }
                buf.clear();
            }
        }
        // Flush remaining
        if !buf.is_empty() {
            let _ = self.send_content_chunk(buf).await;
        }
    }

    /// Send one content delta chunk; returns `false` when the receiver hung up.
    async fn send_content_chunk(&self, content: String) -> bool {
        let chunk = self.chunk(OpenAiDelta::with_content(content), None);
        self.send_chunk(chunk).await
    }

    /// Send all tool calls in a single delta chunk.
    async fn send_tool_call_chunk(&self, tool_calls: &[crate::llm::ToolCall]) {
        let deltas: Vec<OpenAiToolCallDelta> = tool_calls
            .iter()
            .enumerate()
            .map(|(i, tc)| OpenAiToolCallDelta {
                index: i as u32,
                id: Some(tc.id.clone()),
                call_type: Some("function".to_string()),
                function: Some(OpenAiToolCallFunctionDelta {
                    name: Some(tc.name.clone()),
                    arguments: Some(serde_json::to_string(&tc.arguments).unwrap_or_default()),
                }),
            })
            .collect();

        let chunk = self.chunk(OpenAiDelta::with_tool_calls(deltas), None);
        let _ = self.send_chunk(chunk).await;
    }

    /// Send the final chunk carrying the finish reason.
    async fn send_finish_chunk(&self, reason: FinishReason) {
        let chunk = self.chunk(OpenAiDelta::empty(), Some(finish_reason_str(reason)));
        let _ = self.send_chunk(chunk).await;
    }
}
