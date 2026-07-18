//! Axum handlers for `/v1/chat/completions` (non-streaming path) and
//! `/v1/models`.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use super::convert::{
    build_completion_request, build_tool_request, chat_completion_id, convert_messages,
    convert_tool_calls_to_openai, finish_reason_str, map_llm_error, openai_error, unix_timestamp,
    validate_model_name,
};
use super::streaming::handle_streaming;
use super::types::{
    OpenAiChatRequest, OpenAiChatResponse, OpenAiChoice, OpenAiErrorResponse, OpenAiMessage,
    OpenAiUsage,
};
use crate::channels::web::server::GatewayState;

pub async fn chat_completions_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<OpenAiChatRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<OpenAiErrorResponse>)> {
    if !state.chat_rate_limiter.check() {
        return Err(openai_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded. Please try again later.",
            "rate_limit_error",
        ));
    }

    let llm = state.llm_provider.as_ref().ok_or_else(|| {
        openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "LLM provider not configured",
            "server_error",
        )
    })?;

    if req.messages.is_empty() {
        return Err(openai_error(
            StatusCode::BAD_REQUEST,
            "messages must not be empty",
            "invalid_request_error",
        ));
    }
    if let Err(e) = validate_model_name(&req.model) {
        return Err(openai_error(
            StatusCode::BAD_REQUEST,
            e,
            "invalid_request_error",
        ));
    }

    let has_tools = req.tools.as_ref().is_some_and(|t| !t.is_empty());
    let stream = req.stream.unwrap_or(false);
    let requested_model = req.model.clone();

    if stream {
        return handle_streaming(llm.clone(), req, has_tools)
            .await
            .map(IntoResponse::into_response);
    }

    // --- Non-streaming path ---

    let messages = convert_messages(&req.messages)
        .map_err(|e| openai_error(StatusCode::BAD_REQUEST, e, "invalid_request_error"))?;
    let id = chat_completion_id();
    let created = unix_timestamp();

    if has_tools {
        let tool_req = build_tool_request(&req, messages);

        let resp = llm
            .complete_with_tools(tool_req)
            .await
            .map_err(map_llm_error)?;
        let model_name = llm.effective_model_name(Some(requested_model.as_str()));

        let tool_calls_openai = if resp.tool_calls.is_empty() {
            None
        } else {
            Some(convert_tool_calls_to_openai(&resp.tool_calls))
        };

        let response = OpenAiChatResponse {
            id,
            object: "chat.completion",
            created,
            model: model_name,
            choices: vec![OpenAiChoice {
                index: 0,
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: resp.content.clone(),
                    name: None,
                    tool_call_id: None,
                    tool_calls: tool_calls_openai,
                },
                finish_reason: finish_reason_str(resp.finish_reason),
            }],
            usage: OpenAiUsage {
                prompt_tokens: resp.input_tokens,
                completion_tokens: resp.output_tokens,
                total_tokens: resp.input_tokens + resp.output_tokens,
            },
        };

        Ok(Json(response).into_response())
    } else {
        let comp_req = build_completion_request(&req, messages);

        let resp = llm.complete(comp_req).await.map_err(map_llm_error)?;
        let model_name = llm.effective_model_name(Some(requested_model.as_str()));

        let response = OpenAiChatResponse {
            id,
            object: "chat.completion",
            created,
            model: model_name,
            choices: vec![OpenAiChoice {
                index: 0,
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(resp.content),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
                finish_reason: finish_reason_str(resp.finish_reason),
            }],
            usage: OpenAiUsage {
                prompt_tokens: resp.input_tokens,
                completion_tokens: resp.output_tokens,
                total_tokens: resp.input_tokens + resp.output_tokens,
            },
        };

        Ok(Json(response).into_response())
    }
}

pub async fn models_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<OpenAiErrorResponse>)> {
    let llm = state.llm_provider.as_ref().ok_or_else(|| {
        openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "LLM provider not configured",
            "server_error",
        )
    })?;

    let model_name = llm.active_model_name();
    let created = unix_timestamp();

    // Try to fetch available models from the provider
    let models = match llm.list_models().await {
        Ok(names) if !names.is_empty() => names
            .into_iter()
            .map(|name| {
                serde_json::json!({
                    "id": name,
                    "object": "model",
                    "created": created,
                    "owned_by": "axinite"
                })
            })
            .collect(),
        Ok(_) => {
            // Empty list: fall back to active model
            vec![serde_json::json!({
                "id": model_name,
                "object": "model",
                "created": created,
                "owned_by": "axinite"
            })]
        }
        Err(e) => return Err(map_llm_error(e)),
    };

    Ok(Json(serde_json::json!({
        "object": "list",
        "data": models
    })))
}
