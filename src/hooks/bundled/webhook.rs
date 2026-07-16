//! Fire-and-forget outbound webhook hooks compiled from
//! [`OutboundWebhookConfig`], with bounded concurrency and event summaries.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::HeaderMap;
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::hooks::{HookContext, HookError, HookEvent, HookOutcome, HookPoint, NativeHook};

use super::config::{HookBundleError, OutboundWebhookConfig, timeout_from_ms};
use super::net_policy::{
    dispatch_client_for_target, validate_webhook_headers, validate_webhook_url,
};

const DEFAULT_WEBHOOK_PRIORITY: u32 = 300;
const DEFAULT_WEBHOOK_TIMEOUT_MS: u64 = 2000;
const DEFAULT_WEBHOOK_MAX_IN_FLIGHT: usize = 32;

/// Runtime outbound webhook hook.
#[derive(Debug)]
pub(super) struct OutboundWebhookHook {
    name: String,
    points: Vec<HookPoint>,
    client: reqwest::Client,
    url: String,
    headers: HeaderMap,
    timeout: Duration,
    semaphore: Arc<Semaphore>,
}

impl OutboundWebhookHook {
    pub(super) fn from_config(
        source: &str,
        config: OutboundWebhookConfig,
    ) -> Result<(Self, u32), HookBundleError> {
        let scoped_name = format!("{}::{}", source, config.name);

        if config.points.is_empty() {
            return Err(HookBundleError::MissingHookPoints { hook: scoped_name });
        }

        let url = validate_webhook_url(&scoped_name, &config.url)?;
        let headers = validate_webhook_headers(&scoped_name, &config.headers)?;

        let timeout = timeout_from_ms(
            config.timeout_ms.or(Some(DEFAULT_WEBHOOK_TIMEOUT_MS)),
            &scoped_name,
        )?;

        let max_in_flight = config
            .max_in_flight
            .unwrap_or(DEFAULT_WEBHOOK_MAX_IN_FLIGHT);
        if max_in_flight == 0 {
            return Err(HookBundleError::InvalidWebhookMaxInFlight { hook: scoped_name });
        }

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| HookBundleError::InvalidFormat(e.to_string()))?;

        let hook = Self {
            name: scoped_name,
            points: config.points,
            client,
            url: url.to_string(),
            headers,
            timeout,
            semaphore: Arc::new(Semaphore::new(max_in_flight)),
        };

        Ok((hook, config.priority.unwrap_or(DEFAULT_WEBHOOK_PRIORITY)))
    }
}

#[derive(Debug, Serialize)]
struct OutboundWebhookPayload {
    hook: String,
    point: String,
    timestamp: String,
    event: OutboundWebhookEventSummary,
    metadata_present: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum OutboundWebhookEventSummary {
    Inbound {
        channel: String,
        has_thread_id: bool,
        content_length: usize,
    },
    ToolCall {
        tool_name: String,
        context: String,
        parameter_count: usize,
    },
    Outbound {
        channel: String,
        has_thread_id: bool,
        content_length: usize,
    },
    SessionStart,
    SessionEnd,
    ResponseTransform {
        response_length: usize,
    },
}

impl NativeHook for OutboundWebhookHook {
    fn name(&self) -> &str {
        &self.name
    }

    fn hook_points(&self) -> &[HookPoint] {
        &self.points
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    async fn execute<'a>(
        &'a self,
        event: &'a HookEvent,
        ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        let payload = OutboundWebhookPayload {
            hook: self.name.clone(),
            point: event.hook_point().as_str().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            event: summarize_webhook_event(event),
            metadata_present: !ctx.metadata.is_null(),
        };

        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                tracing::warn!(
                    hook = %self.name,
                    "Dropping outbound webhook delivery due to concurrency limit"
                );
                return Ok(HookOutcome::ok());
            }
        };

        let base_client = self.client.clone();
        let url = self.url.clone();
        let headers = self.headers.clone();
        let hook_name = self.name.clone();
        let timeout = self.timeout;

        tokio::spawn(async move {
            let _permit = permit;

            let client = match dispatch_client_for_target(&base_client, &url, timeout).await {
                Ok(client) => client,
                Err(err) => {
                    tracing::warn!(
                        hook = %hook_name,
                        error = %err,
                        "Outbound webhook target blocked by runtime network policy"
                    );
                    return;
                }
            };

            let request = client.post(url).headers(headers).json(&payload);

            if let Err(err) = request.send().await {
                tracing::warn!(
                    hook = %hook_name,
                    error = %err,
                    "Outbound webhook delivery failed"
                );
            }
        });

        Ok(HookOutcome::ok())
    }
}

fn summarize_webhook_event(event: &HookEvent) -> OutboundWebhookEventSummary {
    match event {
        HookEvent::Inbound {
            channel,
            content,
            thread_id,
            ..
        } => OutboundWebhookEventSummary::Inbound {
            channel: channel.clone(),
            has_thread_id: thread_id.is_some(),
            content_length: content.len(),
        },
        HookEvent::ToolCall {
            tool_name,
            context,
            parameters,
            ..
        } => OutboundWebhookEventSummary::ToolCall {
            tool_name: tool_name.clone(),
            context: context.clone(),
            parameter_count: match parameters {
                serde_json::Value::Object(map) => map.len(),
                serde_json::Value::Null => 0,
                _ => 1,
            },
        },
        HookEvent::Outbound {
            channel,
            content,
            thread_id,
            ..
        } => OutboundWebhookEventSummary::Outbound {
            channel: channel.clone(),
            has_thread_id: thread_id.is_some(),
            content_length: content.len(),
        },
        HookEvent::SessionStart { .. } => OutboundWebhookEventSummary::SessionStart,
        HookEvent::SessionEnd { .. } => OutboundWebhookEventSummary::SessionEnd,
        HookEvent::ResponseTransform { response, .. } => {
            OutboundWebhookEventSummary::ResponseTransform {
                response_length: response.len(),
            }
        }
    }
}
