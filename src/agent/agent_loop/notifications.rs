//! Outbound notification helpers for the agent loop.
//!
//! Applies `BeforeOutbound` hooks to responses and forwards self-repair and
//! background-task notifications to the appropriate channels.

use std::sync::Arc;

use crate::agent::self_repair::{RepairNotification, RepairNotificationRoute};
use crate::channels::{ChannelManager, OutgoingResponse};
use crate::hooks::HookRegistry;

/// Reserved user ID for system-generated repair notifications.
pub(super) const SYSTEM_USER_ID: &str = "default";

/// Addressing details for a single outbound response passed through the
/// `BeforeOutbound` hook chain.
pub(super) struct OutboundRoute<'a> {
    /// User the response addresses.
    pub user_id: &'a str,
    /// Channel the response is destined for.
    pub channel: &'a str,
    /// Optional conversation thread within the channel.
    pub thread_id: Option<&'a str>,
}

pub(super) async fn apply_before_outbound_hooks(
    hooks: &Arc<HookRegistry>,
    route: OutboundRoute<'_>,
    response: OutgoingResponse,
) -> Option<OutgoingResponse> {
    let event = crate::hooks::HookEvent::Outbound {
        user_id: route.user_id.to_string(),
        channel: route.channel.to_string(),
        content: response.content.clone(),
        thread_id: route.thread_id.map(str::to_string),
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

/// Routing details for a background-task notification.
pub(super) struct NotificationRoute<'a> {
    /// User the notification addresses.
    pub user: &'a str,
    /// Preferred channel; the notification is broadcast to all channels when
    /// this is absent or the targeted delivery fails.
    pub channel: Option<&'a str>,
    /// Human-readable label used in failure logs (e.g. "heartbeat").
    pub description: &'a str,
}

/// Delivers a notification to its preferred channel, falling back to a
/// broadcast on every channel when no target is configured or the targeted
/// delivery fails.
pub(super) async fn dispatch_notification(
    channels: &Arc<ChannelManager>,
    hooks: &Arc<HookRegistry>,
    route: NotificationRoute<'_>,
    response: OutgoingResponse,
) {
    if try_targeted_send(channels, hooks, &route, response.clone()).await {
        return;
    }
    broadcast_to_all(channels, hooks, &route, response).await;
}

/// Attempts delivery on the preferred channel.
///
/// Returns `true` when no fallback is needed: either no response survived the
/// hook chain, or the targeted broadcast succeeded.
async fn try_targeted_send(
    channels: &Arc<ChannelManager>,
    hooks: &Arc<HookRegistry>,
    route: &NotificationRoute<'_>,
    response: OutgoingResponse,
) -> bool {
    let Some(channel) = route.channel else {
        return false;
    };
    let hook_route = OutboundRoute {
        user_id: route.user,
        channel,
        thread_id: None,
    };
    match apply_before_outbound_hooks(hooks, hook_route, response).await {
        None => true,
        Some(filtered) => channels
            .broadcast(channel, route.user, filtered)
            .await
            .is_ok(),
    }
}

/// Broadcasts a notification on every registered channel, logging failures.
async fn broadcast_to_all(
    channels: &Arc<ChannelManager>,
    hooks: &Arc<HookRegistry>,
    route: &NotificationRoute<'_>,
    response: OutgoingResponse,
) {
    for channel in channels.channel_names().await {
        let hook_route = OutboundRoute {
            user_id: route.user,
            channel: &channel,
            thread_id: None,
        };
        let Some(filtered) = apply_before_outbound_hooks(hooks, hook_route, response.clone()).await
        else {
            continue;
        };
        if let Err(error) = channels.broadcast(&channel, route.user, filtered).await {
            tracing::warn!(
                "Failed to broadcast {} to {}: {}",
                route.description,
                channel,
                error
            );
        }
    }
}

pub(super) async fn forward_repair_notification(
    channels: &Arc<ChannelManager>,
    hooks: &Arc<HookRegistry>,
    notification: RepairNotification,
) {
    let response = OutgoingResponse::text(format!("Self-Repair: {}", notification.message));
    match notification.route {
        RepairNotificationRoute::BroadcastAll { user_id } => {
            let route = NotificationRoute {
                user: &user_id,
                channel: None,
                description: "self-repair notification",
            };
            broadcast_to_all(channels, hooks, &route, response).await;
        }
        RepairNotificationRoute::Broadcast { channel, user_id } => {
            let hook_route = OutboundRoute {
                user_id: &user_id,
                channel: &channel,
                thread_id: None,
            };
            let Some(response) = apply_before_outbound_hooks(hooks, hook_route, response).await
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
