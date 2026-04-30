//! Default self-repair policy implementation.

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::context::{ContextManager, JobRecoveryError};
use crate::db::Database;
use crate::error::RepairError;
#[cfg(any(test, feature = "self_repair_extras"))]
use crate::tools::ToolRegistry;
use crate::tools::builder::{BuildResult, ProjectName};
use crate::tools::{BuildRequirement, Language, SoftwareBuilder, SoftwareType};

use super::traits::NativeSelfRepair;
use super::types::{BrokenTool, RepairResult, StuckJob};

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
    #[cfg(any(test, feature = "self_repair_extras"))]
    tools: Option<Arc<ToolRegistry>>,
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
            #[cfg(any(test, feature = "self_repair_extras"))]
            tools: None,
        }
    }

    /// Add a Store for tool failure tracking.
    pub(crate) fn with_store(mut self, store: Arc<dyn Database>) -> Self {
        self.store = Some(store);
        self
    }

    /// Validates preconditions for tool repair: builder/store availability and attempt limits.
    /// Returns the builder and store references on success, or a terminal RepairResult on failure.
    fn validate_repair_preconditions(
        &self,
        tool: &BrokenTool,
    ) -> Result<BuilderAndDb<'_>, RepairResult> {
        let Some(ref builder) = self.builder else {
            tracing::warn!(
                tool_name = %tool.name,
                "repair precondition failed: builder not available"
            );
            return Err(RepairResult::ManualRequired {
                message: format!("Builder not available for repairing tool '{}'", tool.name),
            });
        };

        let Some(ref store) = self.store else {
            tracing::warn!(
                tool_name = %tool.name,
                "repair precondition failed: store not available"
            );
            return Err(RepairResult::ManualRequired {
                message: "Store not available for tracking repair".to_string(),
            });
        };

        if tool.repair_attempts >= self.max_repair_attempts {
            tracing::warn!(
                tool_name = %tool.name,
                repair_attempts = tool.repair_attempts,
                max_repair_attempts = self.max_repair_attempts,
                "repair precondition failed: max repair attempts exceeded"
            );
            return Err(RepairResult::ManualRequired {
                message: format!(
                    "Tool '{}' exceeded max repair attempts ({})",
                    tool.name, self.max_repair_attempts
                ),
            });
        }

        Ok((builder, store))
    }

    /// Creates a BuildRequirement from a BrokenTool, validating the tool name.
    fn build_repair_requirement(tool: &BrokenTool) -> Result<BuildRequirement, RepairError> {
        let project_name = ProjectName::new(&tool.name).map_err(|error| RepairError::Failed {
            target_type: "tool".to_string(),
            target_id: Uuid::nil(),
            reason: format!(
                "invalid tool name '{}' for repair build: {error}",
                tool.name
            ),
        })?;

        Ok(BuildRequirement {
            name: project_name,
            description: format!(
                concat!(
                    "Repair broken WASM tool.\n\n",
                    "Tool name: {}\n",
                    "Previous error: {}\n",
                    "Failure count: {}\n\n",
                    "Analyze the error, fix the implementation, and rebuild."
                ),
                tool.name,
                tool.last_error.as_deref().unwrap_or("Unknown error"),
                tool.failure_count
            ),
            software_type: SoftwareType::WasmTool,
            language: Language::Rust,
            input_spec: None,
            output_spec: None,
            dependencies: vec![],
            capabilities: vec!["http".to_string(), "workspace".to_string()],
        })
    }

    /// Handles the build result, marking the tool as repaired if successful.
    async fn handle_build_result(
        result: BuildResult,
        tool: &BrokenTool,
        store: &dyn Database,
    ) -> Result<RepairResult, RepairError> {
        if result.success {
            tracing::info!(
                "Successfully rebuilt tool '{}' after {} iterations",
                tool.name,
                result.iterations
            );

            // Mark as repaired in database
            match store.mark_tool_repaired(&tool.name).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!(
                        tool_name = %tool.name,
                        error = %e,
                        "failed to mark tool as repaired in database after successful build"
                    );
                    return Err(RepairError::Failed {
                        target_type: "tool".to_string(),
                        target_id: Uuid::nil(),
                        reason: format!("failed to mark {} as repaired: {}", tool.name, e),
                    });
                }
            }

            // Log if the tool was auto-registered
            if result.registered {
                tracing::info!("Repaired tool '{}' auto-registered", tool.name);
            }

            Ok(RepairResult::Success {
                message: format!(
                    "Tool '{}' repaired successfully after {} {}",
                    tool.name,
                    result.iterations,
                    Self::iteration_word(result.iterations)
                ),
            })
        } else {
            // Build completed but failed
            tracing::warn!(
                "Repair build for '{}' completed but failed: {:?}",
                tool.name,
                result.error
            );
            Ok(RepairResult::Retry {
                message: format!(
                    "Repair attempt {} for '{}' failed: {}",
                    tool.repair_attempts + 1,
                    tool.name,
                    result.error.unwrap_or_else(|| "Unknown error".to_string())
                ),
            })
        }
    }

    async fn attempt_repair_build(
        tool: &BrokenTool,
        store: &dyn Database,
        builder: &dyn SoftwareBuilder,
        requirement: &BuildRequirement,
    ) -> Result<RepairResult, RepairError> {
        match builder.build(requirement).await {
            Ok(result) => Self::handle_build_result(result, tool, store).await,
            Err(e) => {
                tracing::error!("Repair build for '{}' errored: {}", tool.name, e);
                Ok(RepairResult::Retry {
                    message: format!("Repair build error: {}", e),
                })
            }
        }
    }

    fn iteration_word(iterations: u32) -> &'static str {
        if iterations == 1 {
            "iteration"
        } else {
            "iterations"
        }
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
    /// # Concurrency model
    ///
    /// The three private helpers invoked by this method
    /// (`build_repair_requirement`, `attempt_repair_build`,
    /// `handle_build_result`) are static associated functions with no shared
    /// mutable state. Concurrent calls for *different* tools are therefore
    /// safe: each call operates on its own `BrokenTool` and `BuildResult`
    /// values.
    ///
    /// Concurrent calls for the *same* tool are not deduplicated: if two
    /// callers race, both may invoke `store.mark_tool_repaired` and
    /// `store.increment_repair_attempts` for the same tool name. The
    /// database layer is responsible for handling such duplicates (e.g. via
    /// idempotent upsert semantics). Callers that require at-most-once
    /// repair semantics must enforce that at the scheduling layer.
    ///
    /// Cancellation at any `.await` point inside the helper chain is safe:
    /// the helpers hold no locks and make no in-memory writes.
    async fn repair_broken_tool<'a>(
        &'a self,
        tool: &'a BrokenTool,
    ) -> Result<RepairResult, RepairError> {
        // Validate preconditions (builder/store availability, attempt limits)
        let (builder, store) = match self.validate_repair_preconditions(tool) {
            Ok(tuple) => tuple,
            Err(result) => return Ok(result),
        };

        // Create build requirement (validates tool name)
        let requirement = Self::build_repair_requirement(tool)?;

        tracing::info!(
            "Attempting to repair tool '{}' (attempt {})",
            tool.name,
            tool.repair_attempts + 1
        );

        // Increment repair attempts
        store
            .increment_repair_attempts(&tool.name)
            .await
            .map_err(|e| RepairError::Failed {
                target_type: "tool".to_string(),
                target_id: Uuid::nil(),
                reason: format!(
                    "failed to increment repair attempts for {}: {}",
                    tool.name, e
                ),
            })?;

        Self::attempt_repair_build(tool, store.as_ref(), builder.as_ref(), &requirement).await
    }
}

fn duration_since(now: DateTime<Utc>, start: DateTime<Utc>) -> Duration {
    let duration = now.signed_duration_since(start);
    Duration::from_millis(duration.num_milliseconds().max(0) as u64)
}

#[cfg(test)]
#[path = "default_tests.rs"]
mod tests;
