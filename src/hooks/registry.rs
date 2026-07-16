//! Hook registry for managing and executing lifecycle hooks.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::hooks::hook::{Hook, HookContext, HookError, HookEvent, HookFailureMode, HookOutcome};

/// A registered hook with its priority.
struct HookEntry {
    hook: Arc<dyn Hook>,
    priority: u32,
}

/// Registry that manages hooks and executes them at lifecycle points.
///
/// Hooks are executed in priority order (lower number = higher priority).
/// A `Reject` outcome stops the chain immediately.
/// A `Modify` outcome chains through subsequent hooks.
pub struct HookRegistry {
    hooks: RwLock<Vec<HookEntry>>,
}

impl HookRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            hooks: RwLock::new(Vec::new()),
        }
    }

    /// Register a hook with default priority (100).
    pub async fn register(&self, hook: Arc<dyn Hook>) {
        self.register_with_priority(hook, 100).await;
    }

    /// Register a hook with a specific priority.
    ///
    /// Lower priority number = runs first.
    pub async fn register_with_priority(&self, hook: Arc<dyn Hook>, priority: u32) {
        let mut hooks = self.hooks.write().await;
        let hook_name = hook.name().to_string();

        if let Some(existing) = hooks
            .iter_mut()
            .find(|entry| entry.hook.name() == hook_name)
        {
            tracing::warn!(
                hook = %hook_name,
                "Replacing existing hook registration with same name"
            );
            existing.hook = hook;
            existing.priority = priority;
        } else {
            hooks.push(HookEntry { hook, priority });
        }

        hooks.sort_by_key(|e| e.priority);
    }

    /// Unregister a hook by name. Returns `true` if it was found and removed.
    pub async fn unregister(&self, name: &str) -> bool {
        let mut hooks = self.hooks.write().await;
        let before = hooks.len();
        hooks.retain(|e| e.hook.name() != name);
        hooks.len() < before
    }

    /// List all registered hook names (in priority order).
    pub async fn list(&self) -> Vec<String> {
        let hooks = self.hooks.read().await;
        hooks.iter().map(|e| e.hook.name().to_string()).collect()
    }

    /// Run all hooks matching the event's hook point.
    ///
    /// - Hooks run in priority order (lowest first).
    /// - `Reject` stops the chain immediately.
    /// - `Modify` chains the modification through subsequent hooks.
    /// - Timeout/error handling respects each hook's `failure_mode`.
    pub async fn run(&self, event: &HookEvent) -> Result<HookOutcome, HookError> {
        let point = event.hook_point();
        let ctx = HookContext::default();

        // Clone matching hooks and drop the read guard before executing.
        // Each hook can run up to its timeout, so holding the guard would
        // block concurrent register/unregister/run calls.
        let matching: Vec<Arc<dyn Hook>> = {
            let hooks = self.hooks.read().await;
            hooks
                .iter()
                .filter(|e| e.hook.hook_points().contains(&point))
                .map(|e| e.hook.clone())
                .collect()
        };

        if matching.is_empty() {
            return Ok(HookOutcome::ok());
        }

        let mut current_event = event.clone();

        for hook in &matching {
            let timeout = hook.timeout();

            let result = tokio::time::timeout(timeout, hook.execute(&current_event, &ctx)).await;

            match result {
                Ok(Ok(HookOutcome::Reject { reason })) => {
                    tracing::debug!(hook = hook.name(), "Hook rejected: {}", reason);
                    return Err(HookError::Rejected { reason });
                }
                Ok(Ok(HookOutcome::Continue {
                    modified: Some(value),
                })) => {
                    tracing::debug!(hook = hook.name(), "Hook modified content");
                    current_event.apply_modification(&value);
                }
                Ok(Ok(HookOutcome::Continue { modified: None })) => {
                    // No-op, continue chain
                }
                Ok(Err(err)) => match hook.failure_mode() {
                    HookFailureMode::FailOpen => {
                        tracing::warn!(hook = hook.name(), "Hook failed (fail-open): {}", err);
                    }
                    HookFailureMode::FailClosed => {
                        tracing::warn!(hook = hook.name(), "Hook failed (fail-closed): {}", err);
                        return Err(HookError::ExecutionFailed {
                            reason: format!("Hook '{}' failed: {}", hook.name(), err),
                        });
                    }
                },
                Err(_elapsed) => match hook.failure_mode() {
                    HookFailureMode::FailOpen => {
                        tracing::warn!(
                            hook = hook.name(),
                            "Hook timed out (fail-open) after {:?}",
                            timeout
                        );
                    }
                    HookFailureMode::FailClosed => {
                        tracing::warn!(
                            hook = hook.name(),
                            "Hook timed out (fail-closed) after {:?}",
                            timeout
                        );
                        return Err(HookError::Timeout { timeout });
                    }
                },
            }
        }

        // Determine final outcome by comparing with original event
        let modified = extract_content(&current_event);
        let original = extract_content(event);

        if modified != original {
            Ok(HookOutcome::modify(modified))
        } else {
            Ok(HookOutcome::ok())
        }
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the primary content string from a hook event.
fn extract_content(event: &HookEvent) -> String {
    match event {
        HookEvent::Inbound { content, .. } | HookEvent::Outbound { content, .. } => content.clone(),
        HookEvent::ToolCall { parameters, .. } => {
            serde_json::to_string(parameters).unwrap_or_default()
        }
        HookEvent::ResponseTransform { response, .. } => response.clone(),
        HookEvent::SessionStart { session_id, .. } | HookEvent::SessionEnd { session_id, .. } => {
            session_id.clone()
        }
    }
}

#[cfg(test)]
mod tests;
