//! Plan generation and plan-driven execution for job workers.
//!
//! When planning is enabled, the worker asks the reasoning engine for an
//! `ActionPlan` and executes it action by action, falling back to direct
//! tool selection when planning fails or more work remains.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::agent::scheduler::WorkerMessage;
use crate::context::JobState;
use crate::error::Error;
use crate::llm::{ActionPlan, ChatMessage, Reasoning, ReasoningContext, ToolCall, ToolSelection};

use super::{Worker, WorkerLoopOutcome};

/// Mutable state threaded through plan execution: the worker's message
/// receiver, the reasoning engine, and the accumulated reasoning context.
struct PlanSession<'a> {
    /// Channel receiving stop, ping, and user-message signals for the worker.
    rx: &'a mut mpsc::Receiver<WorkerMessage>,
    /// Reasoning engine used to confirm plan completion.
    reasoning: &'a Reasoning,
    /// Conversation context accumulated across the worker loop.
    reason_ctx: &'a mut ReasoningContext,
}

impl Worker {
    /// Generate an execution plan when planning is enabled.
    ///
    /// Returns `None` when planning is disabled or when the planner fails (in
    /// which case a warning is logged and direct tool selection is used
    /// instead).
    async fn generate_plan(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Option<ActionPlan> {
        if !self.use_planning() {
            return None;
        }
        match reasoning.plan(reason_ctx).await {
            Ok(p) => {
                tracing::info!(
                    "Created plan for job {}: {} actions, {:.0}% confidence",
                    self.job_id,
                    p.actions.len(),
                    p.confidence * 100.0
                );
                reason_ctx.messages.push(ChatMessage::assistant(format!(
                    "I've created a plan to accomplish this goal: {}\n\nSteps:\n{}",
                    p.goal,
                    p.actions
                        .iter()
                        .enumerate()
                        .map(|(i, a)| format!("{}. {} - {}", i + 1, a.tool_name, a.reasoning))
                        .collect::<Vec<_>>()
                        .join("\n")
                )));
                self.log_event(
                    "message",
                    serde_json::json!({
                        "role": "assistant",
                        "content": format!(
                            "Plan: {}\n\n{}",
                            p.goal,
                            p.actions
                                .iter()
                                .enumerate()
                                .map(|(i, a)| format!("{}. {} - {}", i + 1, a.tool_name, a.reasoning))
                                .collect::<Vec<_>>()
                                .join("\n")
                        ),
                    }),
                );
                Some(p)
            }
            Err(e) => {
                tracing::warn!(
                    "Planning failed for job {}, falling back to direct selection: {}",
                    self.job_id,
                    e
                );
                None
            }
        }
    }

    /// Run the planning phase and, if a plan is produced, execute it.
    ///
    /// Returns `Ok(Some(outcome))` when the caller should terminate with
    /// `outcome`, or `Ok(None)` when the loop should continue with direct tool
    /// selection.
    pub(super) async fn maybe_plan_and_execute(
        &self,
        rx: &mut mpsc::Receiver<WorkerMessage>,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<WorkerLoopOutcome>, Error> {
        let Some(plan) = self.generate_plan(reasoning, reason_ctx).await else {
            return Ok(None);
        };

        let mut session = PlanSession {
            rx,
            reasoning,
            reason_ctx,
        };
        match self.execute_plan(&mut session, &plan).await? {
            WorkerLoopOutcome::Completed => return Ok(Some(WorkerLoopOutcome::Completed)),
            WorkerLoopOutcome::Exited => return Ok(Some(WorkerLoopOutcome::Exited)),
            WorkerLoopOutcome::ContinueDirectSelection => {}
        }

        if let Ok(ctx) = self.context_manager().get_context(self.job_id).await
            && Self::is_finished_state(ctx.state)
        {
            return Ok(Some(WorkerLoopOutcome::Exited));
        }

        Ok(None)
    }

    /// Whether the job has reached a state where the worker loop should exit.
    fn is_finished_state(state: JobState) -> bool {
        state.is_terminal() || matches!(state, JobState::Stuck | JobState::Completed)
    }

    /// Execute a pre-generated plan.
    async fn execute_plan(
        &self,
        session: &mut PlanSession<'_>,
        plan: &ActionPlan,
    ) -> Result<WorkerLoopOutcome, Error> {
        for (i, action) in plan.actions.iter().enumerate() {
            if let Some(outcome) = self.drain_plan_signals(session.rx, session.reason_ctx) {
                return Ok(outcome);
            }

            tracing::debug!(
                "Job {} executing planned action {}/{}: {} - {}",
                self.job_id,
                i + 1,
                plan.actions.len(),
                action.tool_name,
                action.reasoning
            );

            self.execute_planned_action(session.reason_ctx, plan, i)
                .await?;

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        self.confirm_plan_completion(session.reasoning, session.reason_ctx)
            .await
    }

    /// Drain pending worker messages during plan execution.
    ///
    /// Returns `Some(outcome)` when the plan must be abandoned: `Exited` on a
    /// stop signal, or `ContinueDirectSelection` when a user message arrives
    /// (the message is injected into the reasoning context first). Returns
    /// `None` when execution should proceed with the next planned action.
    fn drain_plan_signals(
        &self,
        rx: &mut mpsc::Receiver<WorkerMessage>,
        reason_ctx: &mut ReasoningContext,
    ) -> Option<WorkerLoopOutcome> {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMessage::Stop => {
                    tracing::debug!(
                        "Worker for job {} received stop signal during plan execution",
                        self.job_id
                    );
                    return Some(WorkerLoopOutcome::Exited);
                }
                WorkerMessage::Ping => {
                    tracing::trace!("Worker for job {} received ping", self.job_id);
                }
                WorkerMessage::Start => {}
                WorkerMessage::UserMessage(content) => {
                    tracing::info!(
                        job_id = %self.job_id,
                        "User message received during plan execution, abandoning plan"
                    );
                    reason_ctx.messages.push(ChatMessage::user(&content));
                    self.log_event(
                        "message",
                        serde_json::json!({
                            "role": "user",
                            "content": content,
                        }),
                    );
                    self.log_event(
                        "status",
                        serde_json::json!({
                            "message": "Plan interrupted by user message, re-evaluating...",
                        }),
                    );
                    return Some(WorkerLoopOutcome::ContinueDirectSelection);
                }
            }
        }
        None
    }

    /// Execute one planned action: record the tool call in the reasoning
    /// context, run the tool, and process its result.
    async fn execute_planned_action(
        &self,
        reason_ctx: &mut ReasoningContext,
        plan: &ActionPlan,
        index: usize,
    ) -> Result<(), Error> {
        let action = &plan.actions[index];
        let selection = ToolSelection {
            tool_name: action.tool_name.clone(),
            parameters: action.parameters.clone(),
            reasoning: action.reasoning.clone(),
            alternatives: vec![],
            tool_call_id: format!("plan_{}_{}", self.job_id, index),
        };

        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                None,
                vec![ToolCall {
                    id: selection.tool_call_id.clone(),
                    name: selection.tool_name.clone(),
                    arguments: selection.parameters.clone(),
                }],
            ));

        let result = self
            .execute_tool(&action.tool_name, &action.parameters)
            .await;

        self.process_tool_result_job(reason_ctx, &selection, result)
            .await
    }

    /// Ask the LLM whether the completed plan finished the job.
    ///
    /// Returns `Completed` when the model signals completion, or
    /// `ContinueDirectSelection` when more work remains.
    async fn confirm_plan_completion(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<WorkerLoopOutcome, Error> {
        reason_ctx.messages.push(ChatMessage::user(
            "All planned actions have been executed. Is the job complete? If not, what else needs to be done?",
        ));

        let response = reasoning.respond(reason_ctx).await?;
        reason_ctx.messages.push(ChatMessage::assistant(&response));

        if crate::util::llm_signals_completion(&response) {
            return Ok(WorkerLoopOutcome::Completed);
        }

        tracing::info!(
            "Job {} plan completed but work remains, falling back to direct selection",
            self.job_id
        );
        self.log_event(
            "status",
            serde_json::json!({
                "message": "Plan completed but job needs more work, continuing...",
            }),
        );
        Ok(WorkerLoopOutcome::ContinueDirectSelection)
    }
}
