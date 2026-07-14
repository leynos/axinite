//! Routine run execution: shared engine context, run finalization,
//! full-job dispatch, and completion notifications.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;
use tokio::sync::mpsc;

use crate::agent::Scheduler;
use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineRun, RunStatus, Trigger, next_cron_fire,
};
use crate::channels::OutgoingResponse;
use crate::config::RoutineConfig;
use crate::db::{Database, RoutineRunCompletion, RoutineRuntimeUpdate};
use crate::error::RoutineError;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::tools::{ApprovalContext, ToolRegistry};
use crate::workspace::Workspace;

use super::lightweight::execute_lightweight;

/// Shared context passed to the execution function.
pub(super) struct EngineContext {
    pub(super) config: RoutineConfig,
    pub(super) store: Arc<dyn Database>,
    pub(super) llm: Arc<dyn LlmProvider>,
    pub(super) workspace: Arc<Workspace>,
    pub(super) notify_tx: mpsc::Sender<OutgoingResponse>,
    pub(super) running_count: Arc<AtomicUsize>,
    pub(super) scheduler: Option<Arc<Scheduler>>,
    pub(super) tools: Arc<ToolRegistry>,
    pub(super) safety: Arc<SafetyLayer>,
}

/// Execute a routine run. Handles both lightweight and full_job modes.
///
/// Note: The caller must pre-increment `running_count` before spawning this task.
/// This ensures the counter is >0 from the moment the task is queued.
pub(super) async fn execute_routine(ctx: EngineContext, routine: Routine, run: RoutineRun) {
    let result = run_action(&ctx, &routine, &run).await;

    // Process result
    let (status, summary, tokens) = match result {
        Ok(execution) => execution,
        Err(e) => {
            tracing::error!(routine = %routine.name, "Execution failed: {}", e);
            (RunStatus::Failed, Some(e.to_string()), None)
        }
    };

    complete_run_record(&ctx, &routine, &run, status, summary.as_deref(), tokens).await;
    update_runtime_state(&ctx, &routine, status).await;
    let thread_id = persist_run_message(&ctx, &routine, &run, status, summary.as_deref()).await;

    // Send notifications based on config
    send_notification(
        &ctx.notify_tx,
        &routine.notify,
        &routine.name,
        status,
        summary.as_deref(),
        thread_id.as_deref(),
    )
    .await;

    // Decrement running count after all finalization steps are complete.
    ctx.running_count.fetch_sub(1, Ordering::Relaxed);
}

/// Dispatches the routine's action to the lightweight or full-job executor.
async fn run_action(
    ctx: &EngineContext,
    routine: &Routine,
    run: &RoutineRun,
) -> Result<(RunStatus, Option<String>, Option<i32>), RoutineError> {
    match &routine.action {
        RoutineAction::Lightweight {
            prompt,
            context_paths,
            max_tokens,
        } => execute_lightweight(ctx, routine, prompt, context_paths, *max_tokens).await,
        RoutineAction::FullJob {
            title,
            description,
            max_iterations,
            tool_permissions,
        } => {
            execute_full_job(
                ctx,
                routine,
                run,
                title,
                description,
                *max_iterations,
                tool_permissions,
            )
            .await
        }
    }
}

/// Marks the run record complete in the store, logging (not propagating)
/// persistence failures.
async fn complete_run_record(
    ctx: &EngineContext,
    routine: &Routine,
    run: &RoutineRun,
    status: RunStatus,
    summary: Option<&str>,
    tokens: Option<i32>,
) {
    if let Err(e) = ctx
        .store
        .complete_routine_run(RoutineRunCompletion {
            id: run.id,
            status,
            result_summary: summary,
            tokens_used: tokens,
        })
        .await
    {
        tracing::error!(routine = %routine.name, "Failed to complete run record: {}", e);
    }
}

/// Updates the routine's runtime bookkeeping: last run, next cron fire,
/// run count, and the consecutive-failure streak.
async fn update_runtime_state(ctx: &EngineContext, routine: &Routine, status: RunStatus) {
    let now = Utc::now();
    let next_fire = if let Trigger::Cron {
        ref schedule,
        ref timezone,
    } = routine.trigger
    {
        next_cron_fire(schedule, timezone.as_deref()).unwrap_or(None)
    } else {
        None
    };

    let new_failures = if status == RunStatus::Failed {
        routine.consecutive_failures + 1
    } else {
        0
    };

    if let Err(e) = ctx
        .store
        .update_routine_runtime(RoutineRuntimeUpdate {
            id: routine.id,
            last_run_at: now,
            next_fire_at: next_fire,
            run_count: routine.run_count + 1,
            consecutive_failures: new_failures,
            state: &routine.state,
        })
        .await
    {
        tracing::error!(routine = %routine.name, "Failed to update runtime state: {}", e);
    }
}

/// Persists the run result to the routine's dedicated conversation thread,
/// returning the thread ID when the conversation could be resolved.
async fn persist_run_message(
    ctx: &EngineContext,
    routine: &Routine,
    run: &RoutineRun,
    status: RunStatus,
    summary: Option<&str>,
) -> Option<String> {
    let conv_id = match ctx
        .store
        .get_or_create_routine_conversation(routine.id, &routine.name, &routine.user_id)
        .await
    {
        Ok(conv_id) => conv_id,
        Err(e) => {
            tracing::error!(routine = %routine.name, "Failed to get routine conversation: {}", e);
            return None;
        }
    };
    tracing::debug!(
        routine = %routine.name,
        routine_id = %routine.id,
        conversation_id = %conv_id,
        "Resolved routine conversation thread"
    );
    // Record the run result as a conversation message
    let msg = match summary {
        Some(s) => format!("[{}] {}: {}", run.trigger_type, status, s),
        None => format!("[{}] {}", run.trigger_type, status),
    };
    if let Err(e) = ctx
        .store
        .add_conversation_message(conv_id, "assistant", &msg)
        .await
    {
        tracing::error!(routine = %routine.name, "Failed to persist routine message: {}", e);
    }
    Some(conv_id.to_string())
}

/// Execute a full-job routine by dispatching to the scheduler.
///
/// Fire-and-forget: creates a job via `Scheduler::dispatch_job` (which handles
/// creation, metadata, persistence, and scheduling), links the routine run to
/// the job, and returns immediately. The job runs independently via the
/// existing Worker/Scheduler with full tool access.
async fn execute_full_job(
    ctx: &EngineContext,
    routine: &Routine,
    run: &RoutineRun,
    title: &str,
    description: &str,
    max_iterations: u32,
    tool_permissions: &[String],
) -> Result<(RunStatus, Option<String>, Option<i32>), RoutineError> {
    let scheduler = ctx
        .scheduler
        .as_ref()
        .ok_or(RoutineError::SchedulerUnavailable)?;

    let mut metadata = serde_json::json!({ "max_iterations": max_iterations });
    // Carry the routine's notify config in job metadata so the message tool
    // can resolve channel/target per-job without global state mutation.
    if let Some(channel) = &routine.notify.channel {
        metadata["notify_channel"] = serde_json::json!(channel);
    }
    metadata["notify_user"] = serde_json::json!(&routine.notify.user);

    // Build approval context: UnlessAutoApproved tools are auto-approved for routines;
    // Always tools require explicit listing in tool_permissions.
    let approval_context = ApprovalContext::autonomous_with_tools(tool_permissions.iter().cloned());

    let job_id = scheduler
        .dispatch_job_with_context(
            crate::agent::scheduler::JobRequest {
                user_id: &routine.user_id,
                title,
                description,
                metadata: Some(metadata),
            },
            approval_context,
        )
        .await
        .map_err(RoutineError::from)?;

    // Link the routine run to the dispatched job
    if let Err(e) = ctx.store.link_routine_run_to_job(run.id, job_id).await {
        tracing::error!(
            routine = %routine.name,
            "Failed to link run to job: {}", e
        );
    }

    tracing::info!(
        routine = %routine.name,
        job_id = %job_id,
        max_iterations = max_iterations,
        "Dispatched full job for routine"
    );

    let summary = format!(
        "Dispatched job {job_id} for full execution with tool access (max_iterations: {max_iterations})"
    );
    Ok((RunStatus::Ok, Some(summary), None))
}

/// Send a notification based on the routine's notify config and run status.
async fn send_notification(
    tx: &mpsc::Sender<OutgoingResponse>,
    notify: &NotifyConfig,
    routine_name: &str,
    status: RunStatus,
    summary: Option<&str>,
    thread_id: Option<&str>,
) {
    let should_notify = match status {
        RunStatus::Ok => notify.on_success,
        RunStatus::Attention => notify.on_attention,
        RunStatus::Failed => notify.on_failure,
        RunStatus::Running => false,
    };

    if !should_notify {
        return;
    }

    let icon = match status {
        RunStatus::Ok => "✅",
        RunStatus::Attention => "🔔",
        RunStatus::Failed => "❌",
        RunStatus::Running => "⏳",
    };

    let message = match summary {
        Some(s) => format!("{} *Routine '{}'*: {}\n\n{}", icon, routine_name, status, s),
        None => format!("{} *Routine '{}'*: {}", icon, routine_name, status),
    };

    let response = OutgoingResponse {
        content: message,
        thread_id: thread_id.map(String::from),
        attachments: Vec::new(),
        metadata: serde_json::json!({
            "source": "routine",
            "routine_name": routine_name,
            "status": status.to_string(),
            "notify_user": notify.user,
            "notify_channel": notify.channel,
        }),
    };

    if let Err(e) = tx.send(response).await {
        tracing::error!(routine = %routine_name, "Failed to send notification: {}", e);
    }
}
