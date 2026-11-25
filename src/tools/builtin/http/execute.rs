//! `NativeTool` implementation for [`HttpTool`]: request building,
//! credential injection, leak detection, and response handling.

use std::collections::HashMap;
use std::time::Duration;

use crate::context::JobContext;
use crate::safety::LeakDetector;
use crate::tools::tool::{ApprovalRequirement, NativeTool, ToolError, ToolOutput, require_str};

#[cfg(feature = "html-to-markdown")]
use crate::tools::builtin::convert_html_to_markdown;

use super::request::{
    attach_body, read_capped_body, reject_oversized_declared_length, request_builder,
    save_body_to_file,
};
use super::validation::{parse_headers_param, validate_url};
use super::{HttpTool, MAX_RESPONSE_SIZE, MAX_SAVE_TO_SIZE};

#[cfg(feature = "html-to-markdown")]
use super::validation::is_html_response;

impl NativeTool for HttpTool {
    fn name(&self) -> &str {
        "http"
    }

    fn description(&self) -> &str {
        "Make HTTP requests to external APIs. Supports GET, POST, PUT, DELETE methods. \
         Use save_to to download binary files (images, PDFs, etc.) to a local path, \
         e.g. {\"method\":\"GET\",\"url\":\"https://picsum.photos/800/600\",\"save_to\":\"/tmp/photo.jpg\"}."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE", "PATCH"],
                    "description": "HTTP method"
                },
                "url": {
                    "type": "string",
                    "description": "The URL to request"
                },
                "headers": {
                    "type": "array",
                    "description": "Optional headers as a list of {name, value} objects",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "value": { "type": "string" }
                        },
                        "required": ["name", "value"],
                        "additionalProperties": false
                    }
                },
                "body": {
                    "description": "Request body (for POST/PUT/PATCH). Can be a JSON object, array, string, or other value."
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Request timeout in seconds (default: 30)"
                },
                "save_to": {
                    "type": "string",
                    "description": "Save response body as raw bytes to this file path instead of returning it. Use for binary downloads (images, PDFs, etc.). The path must be under /tmp/."
                }
            },
            "required": ["method", "url"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let method = require_str(&params, "method")?;

        let url = require_str(&params, "url")?;
        let mut parsed_url = validate_url(url)?;

        // Parse headers
        let mut headers_vec = parse_headers_param(params.get("headers"))?;

        // Build request
        let mut request = request_builder(&self.client, method, &parsed_url)?;

        // Add headers
        for (key, value) in &headers_vec {
            request = request.header(key.as_str(), value.as_str());
        }

        // Add body if present
        let (request, body_bytes) = attach_body(request, params.get("body"))?;

        // Credential injection from shared registry
        let request = self
            .inject_registry_credentials(&ctx.user_id, request, &mut parsed_url, &mut headers_vec)
            .await;

        // Leak detection on outbound request (url/headers/body)
        let detector = LeakDetector::new();
        detector
            .scan_http_request(parsed_url.as_str(), &headers_vec, body_bytes.as_deref())
            .map_err(|e| ToolError::NotAuthorized(format!("{}", e)))?;

        // Build the interceptor request descriptor for recording/replay
        let intercept_req = crate::llm::recording::HttpExchangeRequest {
            method: method.to_uppercase(),
            url: parsed_url.to_string(),
            headers: headers_vec.clone(),
            body: body_bytes
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
        };

        // Check HTTP interceptor (replay mode returns pre-recorded response)
        if let Some(ref interceptor) = ctx.http_interceptor
            && let Some(recorded) = interceptor.before_request(&intercept_req).await
        {
            let headers: HashMap<String, String> = recorded.headers.iter().cloned().collect();
            let body: serde_json::Value = serde_json::from_str(&recorded.body)
                .unwrap_or_else(|_| serde_json::Value::String(recorded.body.clone()));
            let result = serde_json::json!({
                "status": recorded.status,
                "headers": headers,
                "body": body
            });
            return Ok(ToolOutput::success(result, start.elapsed()).with_raw(recorded.body));
        }

        // Execute request
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                ToolError::Timeout(Duration::from_secs(30))
            } else {
                ToolError::ExternalService(e.to_string())
            }
        })?;

        let status = response.status().as_u16();

        // Redirects are followed automatically (up to 10 hops).
        // If we still see a 3xx here, the chain was too long.

        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
            .collect();

        // Use a larger size limit when saving to disk (file downloads)
        let saving_to_disk = params.get("save_to").is_some();
        let max_size = if saving_to_disk {
            MAX_SAVE_TO_SIZE
        } else {
            MAX_RESPONSE_SIZE
        };

        // Pre-check Content-Length header to reject obviously oversized responses
        // before downloading anything, preventing OOM from malicious servers.
        reject_oversized_declared_length(&response, &parsed_url, max_size)?;

        // Stream the response body with a hard size cap.
        let body_bytes = read_capped_body(response, max_size).await?;

        // If save_to is specified, write raw bytes to file and return metadata.
        if let Some(save_to) = params.get("save_to").and_then(|v| v.as_str()) {
            save_body_to_file(save_to, &body_bytes).await?;
            let result = serde_json::json!({
                "status": status,
                "saved_to": save_to,
                "size_bytes": body_bytes.len(),
                "headers": headers});
            return Ok(ToolOutput::success(result, start.elapsed()));
        }

        let body_text = String::from_utf8_lossy(&body_bytes).into_owned();

        // Record the HTTP exchange if interceptor is present (recording mode)
        if let Some(ref interceptor) = ctx.http_interceptor {
            let resp_headers: Vec<(String, String)> = headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            interceptor
                .after_response(
                    &intercept_req,
                    &crate::llm::recording::HttpExchangeResponse {
                        status,
                        headers: resp_headers,
                        body: body_text.clone(),
                    },
                )
                .await;
        }

        #[cfg(feature = "html-to-markdown")]
        let body_text = if is_html_response(&headers) {
            match convert_html_to_markdown(&body_text, parsed_url.as_str()) {
                Ok(md) => md,
                Err(e) => {
                    tracing::warn!(url = %parsed_url, error = %e, "HTML-to-markdown conversion failed, returning raw HTML");
                    body_text
                }
            }
        } else {
            body_text
        };

        // Try to parse as JSON, fall back to string
        let body: serde_json::Value = serde_json::from_str(&body_text)
            .unwrap_or_else(|_| serde_json::Value::String(body_text.clone()));

        let result = serde_json::json!({
            "status": status,
            "headers": headers,
            "body": body
        });

        Ok(ToolOutput::success(result, start.elapsed()).with_raw(body_text))
    }

    fn estimated_duration(&self, _params: &serde_json::Value) -> Option<Duration> {
        Some(Duration::from_secs(5)) // Average HTTP request time
    }

    fn requires_sanitization(&self) -> bool {
        true // External data always needs sanitization
    }

    fn requires_approval(&self, params: &serde_json::Value) -> ApprovalRequirement {
        // 1. Manual auth headers/query params in LLM params
        if crate::safety::params_contain_manual_credentials(params) {
            return ApprovalRequirement::Always;
        }
        // 2. Target host has credential mappings (will be auto-injected)
        if self.host_has_mapped_credentials(params) {
            return ApprovalRequirement::Always;
        }
        // Default: outbound HTTP still needs approval unless auto-approved
        ApprovalRequirement::UnlessAutoApproved
    }

    fn rate_limit_config(&self) -> Option<crate::tools::tool::ToolRateLimitConfig> {
        Some(crate::tools::tool::ToolRateLimitConfig::new(30, 500))
    }
}
