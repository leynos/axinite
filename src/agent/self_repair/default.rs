//! Default self-repair policy implementation.

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};

use crate::context::{ContextManager, JobRecoveryError};
use crate::db::Database;
use crate::error::RepairError;
use crate::tools::SoftwareBuilder;
#[cfg(any(test, feature = "self_repair_extras"))]
use crate::tools::ToolRegistry;

use super::repair_claim::RepairClaims;
use super::traits::NativeSelfRepair;
use super::types::{BrokenTool, RepairResult, StuckJob};

#[path = "default_repair_helpers.rs"]
mod repair_helpers;

/// Tuple of builder and database references for tool repair.
///
/// This alias simplifies the return type of `validate_repair_preconditions`
/// and avoids the need for a clippy type_complexity suppression.
pub(crate) type BuilderAndDb<'a> = (&'a Arc<dyn SoftwareBuilder>, &'a Arc<dyn Database>);

/// Default self-repair implementation.
pub struct DefaultSelfRepair {
    context_manager: Arc<ContextManager>,
    stuck_threshold: Duration,
    max_repair_attempts: u32,
    store: Option<Arc<dyn Database>>,
    builder: Option<Arc<dyn SoftwareBuilder>>,
    repair_claims: RepairClaims,
    #[cfg(any(test, feature = "self_repair_extras"))]
    tools: Option<Arc<ToolRegistry>>,
    /// When set, repair tasks await this barrier immediately before
    /// `claim_tool` so concurrent callers overlap on the claim window.
    #[cfg(test)]
    claim_overlap_barrier: Option<Arc<tokio::sync::Barrier>>,
}

impl DefaultSelfRepair {
    /// Create a new self-repair instance.
    pub fn new(
        context_manager: Arc<ContextManager>,
        stuck_threshold: Duration,
        max_repair_attempts: u32,
    ) -> Self {
        Self {
            context_manager,
            stuck_threshold,
            max_repair_attempts,
            store: None,
            builder: None,
            repair_claims: RepairClaims::default(),
            #[cfg(any(test, feature = "self_repair_extras"))]
            tools: None,
            #[cfg(test)]
            claim_overlap_barrier: None,
        }
    }

    /// Add a Store for tool failure tracking.
    pub(crate) fn with_store(mut self, store: Arc<dyn Database>) -> Self {
        self.store = Some(store);
        self
    }
}

#[cfg(test)]
impl DefaultSelfRepair {
    pub(crate) fn with_claim_overlap_barrier(mut self, barrier: Arc<tokio::sync::Barrier>) -> Self {
        self.claim_overlap_barrier = Some(barrier);
        self
    }
}

/// Extras module for self-repair functionality that is feature-gated.
#[cfg(any(test, feature = "self_repair_extras"))]
mod extras {
    use super::*;

    impl DefaultSelfRepair {
        /// Add a Builder and ToolRegistry for automatic tool repair.
        #[allow(dead_code)]
        pub(crate) fn with_builder(
            mut self,
            builder: Arc<dyn SoftwareBuilder>,
            tools: Arc<ToolRegistry>,
        ) -> Self {
            self.builder = Some(builder);
            self.tools = Some(tools);
            self
        }
    }
}

impl NativeSelfRepair for DefaultSelfRepair {
    async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
        let stuck_contexts = self.context_manager.find_stuck_contexts().await;
        let mut stuck_jobs = Vec::new();
        let now = Utc::now();

        for ctx in stuck_contexts {
            let Some(stuck_since) = ctx.stuck_since() else {
                continue;
            };
            let stuck_duration = duration_since(now, stuck_since);
            if stuck_duration < self.stuck_threshold {
                continue;
            }

            stuck_jobs.push(StuckJob {
                job_id: ctx.job_id,
                stuck_since,
                stuck_duration,
                last_error: None,
                repair_attempts: ctx.repair_attempts,
            });
        }

        stuck_jobs
    }

    async fn repair_stuck_job<'a>(
        &'a self,
        job: &'a StuckJob,
    ) -> Result<RepairResult, RepairError> {
        // Check if we've exceeded max repair attempts
        if job.repair_attempts >= self.max_repair_attempts {
            return Ok(RepairResult::ManualRequired {
                message: format!(
                    "Job {} has exceeded maximum repair attempts ({})",
                    job.job_id, self.max_repair_attempts
                ),
            });
        }

        // Try to recover the job
        let result = self
            .context_manager
            .update_context(job.job_id, |ctx| ctx.attempt_recovery())
            .await;

        match result {
            Ok(Ok(())) => {
                tracing::info!("Successfully recovered job {}", job.job_id);
                Ok(RepairResult::Success {
                    message: format!("Job {} recovered and will be retried", job.job_id),
                })
            }
            Ok(Err(JobRecoveryError::NotStuck)) => {
                tracing::debug!("Job {} already recovered (not stuck)", job.job_id);
                Ok(RepairResult::Success {
                    message: format!("Job {} already recovered", job.job_id),
                })
            }
            Ok(Err(JobRecoveryError::InvariantViolation(reason))) => Err(RepairError::Failed {
                target_type: "job".to_string(),
                target_id: job.job_id,
                reason,
            }),
            Err(e) => Err(RepairError::Failed {
                target_type: "job".to_string(),
                target_id: job.job_id,
                reason: e.to_string(),
            }),
        }
    }

    async fn detect_broken_tools(&self) -> Vec<BrokenTool> {
        let Some(ref store) = self.store else {
            return vec![];
        };

        // Threshold: 5 failures before considering a tool broken
        match store.get_broken_tools(5).await {
            Ok(tools) => {
                if !tools.is_empty() {
                    tracing::info!("Detected {} broken tools needing repair", tools.len());
                }
                tools
            }
            Err(e) => {
                tracing::warn!("Failed to detect broken tools: {}", e);
                vec![]
            }
        }
    }

    /// Attempts to repair a broken tool by building a new version.
    ///
    /// See `docs/developers-guide.md` for the helper concurrency model.
    async fn repair_broken_tool<'a>(
        &'a self,
        tool: &'a BrokenTool,
    ) -> Result<RepairResult, RepairError> {
        let (builder, store) = match self.validate_repair_preconditions(tool) {
            Ok(tuple) => tuple,
            Err(result) => return Ok(result),
        };

        #[cfg(test)]
        if let Some(barrier) = self.claim_overlap_barrier.as_ref() {
            barrier.wait().await;
        }
        let _claim = match self.repair_claims.claim_tool(tool)? {
            Some(claim) => claim,
            None => {
                tracing::warn!(
                    tool_name = %tool.name,
                    "repair precondition failed: tool repair already claimed"
                );
                return Ok(RepairResult::Retry {
                    message: format!("Repair already in progress for '{}'", tool.name),
                });
            }
        };

        let persisted_tool = match Self::load_persisted_broken_tool(store.as_ref(), tool).await {
            Ok(persisted_tool) => persisted_tool,
            Err(e) => {
                tracing::error!(
                    tool_name = %tool.name,
                    error = %e,
                    "failed to load persisted broken tool state"
                );
                return Err(e);
            }
        };
        let tool_for_repair = persisted_tool.as_ref().unwrap_or(tool);
        if let Some(p) = persisted_tool.as_ref() {
            tracing::debug!(
                tool_name = %p.name,
                source = "persisted",
                "using persisted tool state for repair"
            );
        } else {
            tracing::debug!(
                tool_name = %tool.name,
                source = "input",
                "using input tool state for repair"
            );
        }

        self.execute_repair(tool_for_repair, builder.as_ref(), store.as_ref())
            .await
    }
}

fn duration_since(now: DateTime<Utc>, start: DateTime<Utc>) -> Duration {
    let duration = now.signed_duration_since(start);
    Duration::from_millis(duration.num_milliseconds().max(0) as u64)
}

#[cfg(test)]
#[path = "default_tests.rs"]
mod tests;
