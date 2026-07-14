//! Background task spawning for the agent loop.
//!
//! Houses the runtime handle types and the `Agent` methods that spawn
//! long-lived background tasks: self-repair, session pruning, heartbeat,
//! and the routine engine.

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::Agent;
use super::notifications::{
    SYSTEM_USER_ID, apply_before_outbound_hooks, forward_repair_notification,
};
use crate::agent::HeartbeatConfig as AgentHeartbeatConfig;
use crate::agent::heartbeat::spawn_heartbeat;
use crate::agent::routine_engine::{RoutineEngine, spawn_cron_ticker};
use crate::agent::self_repair::{
    DefaultSelfRepair, RepairNotification, RepairNotificationRoute, RepairTask,
};
use crate::channels::OutgoingResponse;

pub(super) struct SelfRepairRuntime {
    pub(super) shutdown_tx: oneshot::Sender<()>,
    pub(super) repair_handle: JoinHandle<()>,
    pub(super) notify_handle: JoinHandle<()>,
}

pub(super) struct RoutineHandles {
    pub(super) cron_handle: JoinHandle<()>,
    pub(super) notify_forwarder: JoinHandle<()>,
    pub(super) engine: Arc<RoutineEngine>,
}

impl Agent {
    pub(super) fn spawn_self_repair(&self) -> SelfRepairRuntime {
        let mut repair = DefaultSelfRepair::new(
            self.context_manager.clone(),
            self.config.stuck_threshold,
            self.config.max_repair_attempts,
        );
        if let Some(store) = self.store() {
            repair = repair.with_store(Arc::clone(store));
        }
        let repair = Arc::new(repair);
        let repair_interval = self.config.repair_check_interval;
        let repair_channels = self.channels.clone();
        let repair_hooks = Arc::clone(self.hooks());
        let (repair_shutdown_tx, repair_shutdown_rx) = oneshot::channel();
        let (repair_notify_tx, mut repair_notify_rx) = mpsc::channel::<RepairNotification>(16);
        let repair_task = RepairTask::new(repair, repair_interval, repair_shutdown_rx)
            .with_notification_tx(
                repair_notify_tx,
                RepairNotificationRoute::BroadcastAll {
                    // System-level repair notices target the reserved system user.
                    user_id: SYSTEM_USER_ID.to_string(),
                },
            );
        let repair_handle = tokio::spawn(repair_task.run());
        let notify_handle = tokio::spawn(async move {
            while let Some(notification) = repair_notify_rx.recv().await {
                forward_repair_notification(&repair_channels, &repair_hooks, notification).await;
            }
        });

        SelfRepairRuntime {
            shutdown_tx: repair_shutdown_tx,
            repair_handle,
            notify_handle,
        }
    }

    pub(super) fn spawn_session_pruning(&self) -> JoinHandle<()> {
        let session_mgr = self.session_manager.clone();
        let session_idle_timeout = self.config.session_idle_timeout;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600)); // Every 10 min
            interval.tick().await; // Skip immediate first tick
            loop {
                interval.tick().await;
                session_mgr.prune_stale_sessions(session_idle_timeout).await;
            }
        })
    }

    pub(super) async fn spawn_heartbeat(&self) -> Option<JoinHandle<()>> {
        let hb_config = self.heartbeat_config.as_ref()?;
        if !hb_config.enabled {
            return None;
        }
        let workspace = match self.workspace() {
            Some(w) => w,
            None => {
                tracing::warn!("Heartbeat enabled but no workspace available");
                return None;
            }
        };
        let mut config = AgentHeartbeatConfig::default()
            .with_interval(std::time::Duration::from_secs(hb_config.interval_secs));
        config.quiet_hours_start = hb_config.quiet_hours_start;
        config.quiet_hours_end = hb_config.quiet_hours_end;
        config.timezone = hb_config
            .timezone
            .clone()
            .or_else(|| Some(self.config.default_timezone.clone()));
        if let (Some(user), Some(channel)) = (&hb_config.notify_user, &hb_config.notify_channel) {
            config = config.with_notify(user, channel);
        }

        // Set up notification channel
        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<OutgoingResponse>(16);

        // Spawn notification forwarder that routes through channel manager
        let notify_channel = hb_config.notify_channel.clone();
        let notify_user = hb_config.notify_user.clone();
        let channels = self.channels.clone();
        let hooks = Arc::clone(self.hooks());
        tokio::spawn(async move {
            while let Some(response) = notify_rx.recv().await {
                let user = notify_user.as_deref().unwrap_or(SYSTEM_USER_ID);

                // Try the configured channel first, fall back to
                // broadcasting on all channels.
                let targeted_ok = if let Some(ref channel) = notify_channel {
                    if let Some(filtered_response) =
                        apply_before_outbound_hooks(&hooks, user, channel, None, response.clone())
                            .await
                    {
                        channels
                            .broadcast(channel, user, filtered_response)
                            .await
                            .is_ok()
                    } else {
                        true
                    }
                } else {
                    false
                };

                if !targeted_ok {
                    for channel in channels.channel_names().await {
                        let Some(filtered_response) = apply_before_outbound_hooks(
                            &hooks,
                            user,
                            &channel,
                            None,
                            response.clone(),
                        )
                        .await
                        else {
                            continue;
                        };
                        if let Err(e) = channels.broadcast(&channel, user, filtered_response).await
                        {
                            tracing::warn!("Failed to broadcast heartbeat to {}: {}", channel, e);
                        }
                    }
                }
            }
        });

        let hygiene = self
            .hygiene_config
            .as_ref()
            .map(|h| h.to_workspace_config())
            .unwrap_or_default();

        Some(spawn_heartbeat(
            config,
            hygiene,
            workspace.clone(),
            self.cheap_llm().clone(),
            Some(notify_tx),
            self.store().map(Arc::clone),
        ))
    }

    pub(super) async fn spawn_routine_engine(&self) -> Option<RoutineHandles> {
        let rt_config = self.routine_config.as_ref()?;
        if !rt_config.enabled {
            return None;
        }
        let (store, workspace) = match (self.store(), self.workspace()) {
            (Some(s), Some(w)) => (s, w),
            _ => {
                tracing::warn!("Routines enabled but store/workspace not available");
                return None;
            }
        };
        // Set up notification channel (same pattern as heartbeat)
        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<OutgoingResponse>(32);

        let engine = Arc::new(RoutineEngine::new(
            rt_config.clone(),
            Arc::clone(store),
            self.llm().clone(),
            Arc::clone(workspace),
            notify_tx,
            Some(self.scheduler.clone()),
            self.tools().clone(),
            self.safety().clone(),
        ));

        // Register routine tools
        self.deps
            .tools
            .register_routine_tools(Arc::clone(store), Arc::clone(&engine));

        // Load initial event cache
        engine.refresh_event_cache().await;

        // Spawn notification forwarder (mirrors heartbeat pattern)
        let channels = self.channels.clone();
        let hooks = Arc::clone(self.hooks());
        let notify_forwarder = tokio::spawn(async move {
            while let Some(response) = notify_rx.recv().await {
                let user = response
                    .metadata
                    .get("notify_user")
                    .and_then(|v| v.as_str())
                    .unwrap_or(SYSTEM_USER_ID)
                    .to_string();
                let notify_channel = response
                    .metadata
                    .get("notify_channel")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Try the configured channel first, fall back to
                // broadcasting on all channels.
                let targeted_ok = if let Some(ref channel) = notify_channel {
                    if let Some(filtered_response) =
                        apply_before_outbound_hooks(&hooks, &user, channel, None, response.clone())
                            .await
                    {
                        channels
                            .broadcast(channel, &user, filtered_response)
                            .await
                            .is_ok()
                    } else {
                        true
                    }
                } else {
                    false
                };

                if !targeted_ok {
                    for channel in channels.channel_names().await {
                        let Some(filtered_response) = apply_before_outbound_hooks(
                            &hooks,
                            &user,
                            &channel,
                            None,
                            response.clone(),
                        )
                        .await
                        else {
                            continue;
                        };
                        if let Err(e) = channels.broadcast(&channel, &user, filtered_response).await
                        {
                            tracing::warn!(
                                "Failed to broadcast routine notification to {}: {}",
                                channel,
                                e
                            );
                        }
                    }
                }
            }
        });

        // Spawn cron ticker
        let cron_interval = std::time::Duration::from_secs(rt_config.cron_check_interval_secs);
        let cron_handle = spawn_cron_ticker(Arc::clone(&engine), cron_interval);

        // Store engine reference for event trigger checking
        let engine_ref = Arc::clone(&engine);
        // `run()` consumes self, so cloning the engine into a local keeps it
        // available for the message loop without changing ownership semantics.

        // Expose engine to gateway for manual triggering
        if let Some(ref slot) = self.routine_engine_slot {
            *slot.write().await = Some(Arc::clone(&engine));
        }

        tracing::debug!(
            "Routines enabled: cron ticker every {}s, max {} concurrent",
            rt_config.cron_check_interval_secs,
            rt_config.max_concurrent_routines
        );

        Some(RoutineHandles {
            cron_handle,
            notify_forwarder,
            engine: engine_ref,
        })
    }
}
