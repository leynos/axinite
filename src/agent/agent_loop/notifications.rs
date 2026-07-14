//! Outbound notification helpers for the agent loop.
//!
//! Applies `BeforeOutbound` hooks to responses and forwards self-repair
//! notifications to the appropriate channels.

use std::sync::Arc;

use crate::agent::self_repair::{RepairNotification, RepairNotificationRoute};
use crate::channels::{ChannelManager, OutgoingResponse};
use crate::hooks::HookRegistry;

/// Reserved user ID for system-generated repair notifications.
pub(super) const SYSTEM_USER_ID: &str = "default";

pub(super) async fn apply_before_outbound_hooks(
    hooks: &Arc<HookRegistry>,
    user_id: &str,
    channel: &str,
    thread_id: Option<&str>,
    response: OutgoingResponse,
) -> Option<OutgoingResponse> {
    let event = crate::hooks::HookEvent::Outbound {
        user_id: user_id.to_string(),
        channel: channel.to_string(),
        content: response.content.clone(),
        thread_id: thread_id.map(str::to_string),
    };
    match hooks.run(&event).await {
        Err(crate::hooks::HookError::Rejected { reason }) => {
            tracing::warn!("BeforeOutbound hook blocked response: {}", reason);
            None
        }
        Err(err) => {
            tracing::warn!("BeforeOutbound hook failed open: {}", err);
            Some(response)
        }
        Ok(crate::hooks::HookOutcome::Continue {
            modified: Some(new_content),
        }) => Some(OutgoingResponse {
            content: new_content,
            ..response
        }),
        Ok(crate::hooks::HookOutcome::Continue { modified: None }) => Some(response),
        Ok(crate::hooks::HookOutcome::Reject { reason }) => {
            tracing::warn!("BeforeOutbound hook blocked response: {}", reason);
            None
        }
    }
}

pub(super) async fn forward_repair_notification(
    channels: &Arc<ChannelManager>,
    hooks: &Arc<HookRegistry>,
    notification: RepairNotification,
) {
    match notification.route {
        RepairNotificationRoute::BroadcastAll { user_id } => {
            let response = OutgoingResponse::text(format!("Self-Repair: {}", notification.message));
            for channel in channels.channel_names().await {
                let Some(filtered_response) =
                    apply_before_outbound_hooks(hooks, &user_id, &channel, None, response.clone())
                        .await
                else {
                    continue;
                };
                if let Err(error) = channels
                    .broadcast(&channel, &user_id, filtered_response)
                    .await
                {
                    tracing::warn!(
                        "Failed to broadcast self-repair notification to {}: {}",
                        channel,
                        error
                    );
                }
            }
        }
        RepairNotificationRoute::Broadcast { channel, user_id } => {
            let response = OutgoingResponse::text(format!("Self-Repair: {}", notification.message));
            let Some(response) =
                apply_before_outbound_hooks(hooks, &user_id, &channel, None, response).await
            else {
                return;
            };
            if let Err(error) = channels.broadcast(&channel, &user_id, response).await {
                tracing::warn!(
                    "Failed to broadcast self-repair notification to {}: {}",
                    channel,
                    error
                );
            }
        }
    }
}
