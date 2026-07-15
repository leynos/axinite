//! Trigger evaluation for the routine engine: event cache refresh, message
//! and system-event matching, and cron due-check firing.

use std::sync::atomic::Ordering;

use regex::Regex;

use crate::agent::routine::Trigger;
use crate::channels::IncomingMessage;

use super::engine::{EventMatcher, RoutineEngine};

impl RoutineEngine {
    /// Refresh the in-memory event trigger cache from DB.
    pub async fn refresh_event_cache(&self) {
        match self.store.list_event_routines().await {
            Ok(routines) => {
                let mut cache = Vec::new();
                for routine in routines {
                    match &routine.trigger {
                        Trigger::Event { pattern, .. } => match Regex::new(pattern) {
                            Ok(re) => cache.push(EventMatcher::Message {
                                routine: routine.clone(),
                                regex: re,
                            }),
                            Err(e) => {
                                tracing::warn!(
                                    routine = %routine.name,
                                    "Invalid event regex '{}': {}",
                                    pattern, e
                                );
                            }
                        },
                        Trigger::SystemEvent { .. } => {
                            cache.push(EventMatcher::System {
                                routine: routine.clone(),
                            });
                        }
                        _ => {}
                    }
                }
                let count = cache.len();
                *self.event_cache.write().await = cache;
                tracing::trace!("Refreshed event cache: {} routines", count);
            }
            Err(e) => {
                tracing::error!("Failed to refresh event cache: {}", e);
            }
        }
    }

    /// Check incoming message against event triggers. Returns number of routines fired.
    ///
    /// Called synchronously from the main loop after handle_message(). The actual
    /// execution is spawned async so this returns quickly.
    pub async fn check_event_triggers(&self, message: &IncomingMessage) -> usize {
        let cache = self.event_cache.read().await;
        let mut fired = 0;

        for matcher in cache.iter() {
            let (routine, re) = match matcher {
                EventMatcher::Message { routine, regex } => (routine, regex),
                EventMatcher::System { .. } => continue,
            };
            // Channel filter
            if let Trigger::Event {
                channel: Some(ch), ..
            } = &routine.trigger
                && ch != &message.channel
            {
                continue;
            }

            // Regex match
            if !re.is_match(&message.content) {
                continue;
            }

            // Cooldown check
            if !self.check_cooldown(routine) {
                tracing::trace!(routine = %routine.name, "Skipped: cooldown active");
                continue;
            }

            // Concurrent run check
            if !self.check_concurrent(routine).await {
                tracing::trace!(routine = %routine.name, "Skipped: max concurrent reached");
                continue;
            }

            // Global capacity check (atomic check-and-increment)
            if !self.try_reserve_running_slot() {
                tracing::warn!(routine = %routine.name, "Skipped: global max concurrent reached");
                continue;
            }

            let detail = truncate(&message.content, 200);
            self.spawn_fire_reserved(routine.clone(), "event", Some(detail));
            fired += 1;
        }

        fired
    }

    /// Emit a structured event to system-event routines.
    ///
    /// Returns the number of routines that were fired.
    pub async fn emit_system_event(
        &self,
        source: &str,
        event_type: &str,
        payload: &serde_json::Value,
        user_id: Option<&str>,
    ) -> usize {
        let event = SystemEventRef {
            source,
            event_type,
            payload,
            user_id,
        };
        let cache = self.event_cache.read().await;
        let mut fired = 0;

        for matcher in cache.iter() {
            let routine = match matcher {
                EventMatcher::System { routine } => routine,
                EventMatcher::Message { .. } => continue,
            };

            if !matches_system_event(routine, &event) {
                continue;
            }

            if !self.passes_system_event_guardrails(routine).await {
                continue;
            }

            let detail = truncate(&format!("{source}:{event_type}"), 200);
            self.spawn_fire_reserved(routine.clone(), "system_event", Some(detail));
            fired += 1;
        }

        fired
    }

    /// Checks cooldown, per-routine concurrency, and global capacity for a
    /// matched system-event routine.
    ///
    /// Returns `true` when a running slot has been reserved and the routine
    /// may fire.
    async fn passes_system_event_guardrails(
        &self,
        routine: &crate::agent::routine::Routine,
    ) -> bool {
        if !self.check_cooldown(routine) {
            tracing::debug!(routine = %routine.name, "Skipped: cooldown active");
            return false;
        }

        if !self.check_concurrent(routine).await {
            tracing::debug!(routine = %routine.name, "Skipped: max concurrent reached");
            return false;
        }

        // Global capacity check (atomic check-and-increment)
        if !self.try_reserve_running_slot() {
            tracing::warn!(routine = %routine.name, "Skipped: global max concurrent reached");
            return false;
        }
        true
    }

    /// Check all due cron routines and fire them. Called by the cron ticker.
    pub async fn check_cron_triggers(&self) {
        let routines = match self.store.list_due_cron_routines().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to load due cron routines: {}", e);
                return;
            }
        };

        for routine in routines {
            // Global capacity check (atomic check-and-increment)
            if !self.try_reserve_running_slot() {
                tracing::warn!("Global max concurrent routines reached, skipping remaining");
                break;
            }

            if !self.check_cooldown(&routine) {
                // Roll back the reservation since this routine won't fire.
                self.running_count.fetch_sub(1, Ordering::Relaxed);
                continue;
            }

            if !self.check_concurrent(&routine).await {
                // Roll back the reservation since this routine won't fire.
                self.running_count.fetch_sub(1, Ordering::Relaxed);
                continue;
            }

            let detail = if let Trigger::Cron { ref schedule, .. } = routine.trigger {
                Some(schedule.clone())
            } else {
                None
            };

            self.spawn_fire_reserved(routine, "cron", detail);
        }
    }
}

/// Borrowed view of an emitted system event, as delivered to the matcher.
struct SystemEventRef<'a> {
    source: &'a str,
    event_type: &'a str,
    payload: &'a serde_json::Value,
    user_id: Option<&'a str>,
}

/// Returns `true` when the routine's system-event trigger matches the
/// emitted event's source, type, user scope, and payload filters.
fn matches_system_event(
    routine: &crate::agent::routine::Routine,
    event: &SystemEventRef<'_>,
) -> bool {
    let Trigger::SystemEvent {
        source: expected_source,
        event_type: expected_event,
        filters,
    } = &routine.trigger
    else {
        return false;
    };

    if !expected_source.eq_ignore_ascii_case(event.source) {
        return false;
    }
    if !expected_event.eq_ignore_ascii_case(event.event_type) {
        return false;
    }
    if !user_matches(routine, event.user_id) {
        return false;
    }
    filters_match(routine, filters, event.payload)
}

/// Returns `true` when the routine belongs to the emitting user, or when no
/// user scoping was requested.
fn user_matches(routine: &crate::agent::routine::Routine, user_id: Option<&str>) -> bool {
    match user_id {
        Some(uid) => routine.user_id == uid,
        None => true,
    }
}

/// Returns `true` when every trigger filter key is present in the payload
/// and matches its expected value (case-insensitively).
fn filters_match(
    routine: &crate::agent::routine::Routine,
    filters: &std::collections::HashMap<String, String>,
    payload: &serde_json::Value,
) -> bool {
    for (key, expected) in filters {
        let Some(actual) = payload
            .get(key)
            .and_then(crate::agent::routine::json_value_as_filter_string)
        else {
            tracing::debug!(routine = %routine.name, filter_key = %key, "Filter key not found in payload");
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
    }
    true
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = crate::util::floor_char_boundary(s, max);
        format!("{}...", &s[..end])
    }
}
