//! Core `RoutineEngine` type: construction, manual firing, run spawning,
//! and guardrail checks (cooldown, concurrency, global capacity).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chrono::Utc;
use regex::Regex;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::agent::Scheduler;
use crate::agent::routine::{Routine, RoutineRun, RunStatus};
use crate::channels::OutgoingResponse;
use crate::config::RoutineConfig;
use crate::db::Database;
use crate::error::RoutineError;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

use super::execution::{EngineContext, execute_routine};

pub(super) enum EventMatcher {
    Message { routine: Routine, regex: Regex },
    System { routine: Routine },
}

/// The routine execution engine.
pub struct RoutineEngine {
    pub(super) config: RoutineConfig,
    pub(super) store: Arc<dyn Database>,
    pub(super) llm: Arc<dyn LlmProvider>,
    pub(super) workspace: Arc<Workspace>,
    /// Sender for notifications (routed to channel manager).
    pub(super) notify_tx: mpsc::Sender<OutgoingResponse>,
    /// Currently running routine count (across all routines).
    pub(super) running_count: Arc<AtomicUsize>,
    /// Cached matchers for all event-driven routines.
    pub(super) event_cache: Arc<RwLock<Vec<EventMatcher>>>,
    /// Scheduler for dispatching jobs (FullJob mode).
    pub(super) scheduler: Option<Arc<Scheduler>>,
    /// Tool registry for lightweight routine tool execution.
    pub(super) tools: Arc<ToolRegistry>,
    /// Safety layer for tool output sanitization.
    pub(super) safety: Arc<SafetyLayer>,
}

impl RoutineEngine {
    #[expect(
        clippy::too_many_arguments,
        reason = "requires multiple collaborator dependencies for engine initialization"
    )]
    pub fn new(
        config: RoutineConfig,
        store: Arc<dyn Database>,
        llm: Arc<dyn LlmProvider>,
        workspace: Arc<Workspace>,
        notify_tx: mpsc::Sender<OutgoingResponse>,
        scheduler: Option<Arc<Scheduler>>,
        tools: Arc<ToolRegistry>,
        safety: Arc<SafetyLayer>,
    ) -> Self {
        Self {
            config,
            store,
            llm,
            workspace,
            notify_tx,
            running_count: Arc::new(AtomicUsize::new(0)),
            event_cache: Arc::new(RwLock::new(Vec::new())),
            scheduler,
            tools,
            safety,
        }
    }

    /// Fire a routine manually (from tool call or CLI).
    ///
    /// Bypasses cooldown checks (those only apply to cron/event triggers).
    /// Still enforces enabled check and concurrent run limit.
    pub async fn fire_manual(
        &self,
        routine_id: Uuid,
        user_id: Option<&str>,
    ) -> Result<Uuid, RoutineError> {
        let routine = self
            .store
            .get_routine(routine_id)
            .await
            .map_err(RoutineError::from)?
            .ok_or(RoutineError::NotFound { id: routine_id })?;

        // Enforce ownership when a user_id is provided (gateway calls).
        if let Some(uid) = user_id
            && routine.user_id != uid
        {
            return Err(RoutineError::NotAuthorized { id: routine_id });
        }

        if !routine.enabled {
            return Err(RoutineError::Disabled {
                name: routine.name.clone(),
            });
        }

        if !self.check_concurrent(&routine).await {
            return Err(RoutineError::MaxConcurrent {
                name: routine.name.clone(),
            });
        }

        let run_id = Uuid::new_v4();
        let run = RoutineRun {
            id: run_id,
            routine_id: routine.id,
            trigger_type: "manual".to_string(),
            trigger_detail: None,
            started_at: Utc::now(),
            completed_at: None,
            status: RunStatus::Running,
            result_summary: None,
            tokens_used: None,
            job_id: None,
            created_at: Utc::now(),
        };

        // Pre-increment running count to prevent race between spawn and task start.
        self.running_count.fetch_add(1, Ordering::Relaxed);

        self.store.create_routine_run(&run).await.map_err(|e| {
            // Roll back the pre-increment since the run will not proceed.
            self.running_count.fetch_sub(1, Ordering::Relaxed);
            RoutineError::from(e)
        })?;

        // Build engine context for execution.
        let engine = EngineContext {
            config: self.config.clone(),
            store: self.store.clone(),
            llm: self.llm.clone(),
            workspace: self.workspace.clone(),
            notify_tx: self.notify_tx.clone(),
            running_count: self.running_count.clone(),
            scheduler: self.scheduler.clone(),
            tools: self.tools.clone(),
            safety: self.safety.clone(),
        };

        tokio::spawn(async move {
            execute_routine(engine, routine, run).await;
        });

        Ok(run_id)
    }

    /// Spawn a fire in a background task.
    ///
    /// The caller must have already reserved a running slot via
    /// `try_reserve_running_slot` or equivalent pre-increment.
    pub(super) fn spawn_fire_reserved(
        &self,
        routine: Routine,
        trigger_type: &str,
        trigger_detail: Option<String>,
    ) {
        let run = RoutineRun {
            id: Uuid::new_v4(),
            routine_id: routine.id,
            trigger_type: trigger_type.to_string(),
            trigger_detail,
            started_at: Utc::now(),
            completed_at: None,
            status: RunStatus::Running,
            result_summary: None,
            tokens_used: None,
            job_id: None,
            created_at: Utc::now(),
        };

        let engine = EngineContext {
            config: self.config.clone(),
            store: self.store.clone(),
            llm: self.llm.clone(),
            workspace: self.workspace.clone(),
            notify_tx: self.notify_tx.clone(),
            running_count: self.running_count.clone(),
            scheduler: self.scheduler.clone(),
            tools: self.tools.clone(),
            safety: self.safety.clone(),
        };

        // Record the run in DB, then spawn execution
        let store = self.store.clone();
        let running_count = self.running_count.clone();
        tokio::spawn(async move {
            if let Err(e) = store.create_routine_run(&run).await {
                tracing::error!(routine = %routine.name, "Failed to record run: {}", e);
                // Decrement on early-return since execute_routine won't be called
                running_count.fetch_sub(1, Ordering::Relaxed);
                return;
            }
            execute_routine(engine, routine, run).await;
        });
    }

    pub(super) fn check_cooldown(&self, routine: &Routine) -> bool {
        if let Some(last_run) = routine.last_run_at {
            let elapsed = Utc::now().signed_duration_since(last_run);
            let cooldown = chrono::Duration::from_std(routine.guardrails.cooldown)
                .unwrap_or(chrono::Duration::seconds(300));
            if elapsed < cooldown {
                return false;
            }
        }
        true
    }

    pub(super) async fn check_concurrent(&self, routine: &Routine) -> bool {
        match self.store.count_running_routine_runs(routine.id).await {
            Ok(count) => count < routine.guardrails.max_concurrent as i64,
            Err(e) => {
                tracing::error!(
                    routine = %routine.name,
                    "Failed to check concurrent runs: {}", e
                );
                false
            }
        }
    }

    /// Atomically check global capacity and reserve a running slot.
    ///
    /// Returns `true` if the current count was below `max_concurrent_routines`
    /// and has been incremented; returns `false` without mutating if at capacity.
    pub(super) fn try_reserve_running_slot(&self) -> bool {
        self.running_count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current < self.config.max_concurrent_routines {
                    Some(current + 1)
                } else {
                    None
                }
            })
            .is_ok()
    }
}

#[cfg(any(test, feature = "test-helpers"))]
impl RoutineEngine {
    /// Returns the current number of in-flight routine tasks.
    ///
    /// Intended for test synchronization only.
    pub fn running_count(&self) -> usize {
        self.running_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Spawn the cron ticker background task.
pub fn spawn_cron_ticker(
    engine: Arc<RoutineEngine>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip immediate first tick
        ticker.tick().await;

        loop {
            ticker.tick().await;
            engine.check_cron_triggers().await;
        }
    })
}
