//! Default self-repair policy implementation.

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::context::ContextManager;
use crate::db::Database;
use crate::error::RepairError;
use crate::tools::builder::ProjectName;
use crate::tools::{BuildRequirement, Language, SoftwareBuilder, SoftwareType, ToolRegistry};

use super::traits::NativeSelfRepair;
use super::types::{BrokenTool, RepairResult, StuckJob};

/// Default self-repair implementation.
pub struct DefaultSelfRepair {
    context_manager: Arc<ContextManager>,
    stuck_threshold: Duration,
    max_repair_attempts: u32,
    store: Option<Arc<dyn Database>>,
    builder: Option<Arc<dyn SoftwareBuilder>>,
    // TODO: use for tool hot-reload after repair
    #[allow(dead_code)]
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
            tools: None,
        }
    }

    /// Add a Store for tool failure tracking.
    #[allow(dead_code)] // TODO: wire up in main.rs when persistence is needed
    pub(crate) fn with_store(mut self, store: Arc<dyn Database>) -> Self {
        self.store = Some(store);
        self
    }

    /// Add a Builder and ToolRegistry for automatic tool repair.
    #[allow(dead_code)] // TODO: wire up in main.rs when auto-repair is needed
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
                last_activity: stuck_since,
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
            Ok(Err(e)) => {
                tracing::warn!("Failed to recover job {}: {}", job.job_id, e);
                Ok(RepairResult::Retry {
                    message: format!("Recovery attempt failed: {}", e),
                })
            }
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

    async fn repair_broken_tool<'a>(
        &'a self,
        tool: &'a BrokenTool,
    ) -> Result<RepairResult, RepairError> {
        let Some(ref builder) = self.builder else {
            return Ok(RepairResult::ManualRequired {
                message: format!("Builder not available for repairing tool '{}'", tool.name),
            });
        };

        let Some(ref store) = self.store else {
            return Ok(RepairResult::ManualRequired {
                message: "Store not available for tracking repair".to_string(),
            });
        };

        // Check repair attempt limit
        if tool.repair_attempts >= self.max_repair_attempts {
            return Ok(RepairResult::ManualRequired {
                message: format!(
                    "Tool '{}' exceeded max repair attempts ({})",
                    tool.name, self.max_repair_attempts
                ),
            });
        }

        tracing::info!(
            "Attempting to repair tool '{}' (attempt {})",
            tool.name,
            tool.repair_attempts + 1
        );

        // Increment repair attempts
        if let Err(e) = store.increment_repair_attempts(&tool.name).await {
            tracing::warn!("Failed to increment repair attempts: {}", e);
        }

        // Create BuildRequirement for repair
        let requirement = BuildRequirement {
            name: ProjectName::new(&tool.name).map_err(|error| RepairError::Failed {
                target_type: "tool".to_string(),
                target_id: Uuid::nil(),
                reason: format!(
                    "invalid tool name '{}' for repair build: {error}",
                    tool.name
                ),
            })?,
            description: format!(
                "Repair broken WASM tool.\n\n\
                 Tool name: {}\n\
                 Previous error: {}\n\
                 Failure count: {}\n\n\
                 Analyze the error, fix the implementation, and rebuild.",
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
        };

        // Attempt to build/repair
        match builder.build(&requirement).await {
            Ok(result) if result.success => {
                tracing::info!(
                    "Successfully rebuilt tool '{}' after {} iterations",
                    tool.name,
                    result.iterations
                );

                // Mark as repaired in database
                if let Err(e) = store.mark_tool_repaired(&tool.name).await {
                    tracing::warn!("Failed to mark tool as repaired: {}", e);
                }

                // Log if the tool was auto-registered
                if result.registered {
                    tracing::info!("Repaired tool '{}' auto-registered", tool.name);
                }

                Ok(RepairResult::Success {
                    message: format!(
                        "Tool '{}' repaired successfully after {} iterations",
                        tool.name, result.iterations
                    ),
                })
            }
            Ok(result) => {
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
            Err(e) => {
                tracing::error!("Repair build for '{}' errored: {}", tool.name, e);
                Ok(RepairResult::Retry {
                    message: format!("Repair build error: {}", e),
                })
            }
        }
    }
}

fn duration_since(now: DateTime<Utc>, start: DateTime<Utc>) -> Duration {
    let duration = now.signed_duration_since(start);
    Duration::from_secs(duration.num_seconds().max(0) as u64)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;

    use super::{BrokenTool, DefaultSelfRepair, RepairResult, StuckJob};
    use crate::agent::self_repair::NativeSelfRepair;
    use crate::context::{ContextManager, JobState};

    // === QA Plan - Self-repair stuck job tests ===

    #[tokio::test]
    async fn detect_no_stuck_jobs_when_all_healthy() {
        let cm = Arc::new(ContextManager::new(10));

        // Create a job and leave it Pending (not stuck).
        cm.create_job("Job 1", "desc").await.unwrap();

        let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3);
        let stuck = NativeSelfRepair::detect_stuck_jobs(&repair).await;
        assert!(stuck.is_empty());
    }

    #[tokio::test]
    async fn detect_stuck_job_finds_stuck_state() {
        let cm = Arc::new(ContextManager::new(10));
        let job_id = cm.create_job("Stuck job", "desc").await.unwrap();

        // Transition to InProgress, then to Stuck.
        cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
            .await
            .unwrap()
            .unwrap();
        cm.update_context(job_id, |ctx| {
            ctx.transition_to(JobState::Stuck, Some("timed out".to_string()))
        })
        .await
        .unwrap()
        .unwrap();

        let repair = DefaultSelfRepair::new(cm, Duration::from_secs(0), 3);
        let stuck = NativeSelfRepair::detect_stuck_jobs(&repair).await;
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].job_id, job_id);
    }

    #[tokio::test]
    async fn detect_stuck_jobs_uses_stuck_threshold_from_latest_stuck_transition() {
        let cm = Arc::new(ContextManager::new(10));
        let job_id = cm
            .create_job("Stuck job", "desc")
            .await
            .expect("failed to await create_job");

        cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
            .await
            .expect("failed to await update_context")
            .expect("expected in-progress transition to succeed");
        cm.update_context(job_id, |ctx| ctx.transition_to(JobState::Stuck, None))
            .await
            .expect("failed to await update_context")
            .expect("expected stuck transition to succeed");
        cm.update_context(job_id, |ctx| {
            let stuck_since = Utc::now() - chrono::Duration::seconds(30);
            let Some(last_transition) = ctx.transitions.last_mut() else {
                return Err("missing stuck transition".to_string());
            };
            last_transition.timestamp = stuck_since;
            Ok(())
        })
        .await
        .expect("failed to await update_context")
        .expect("expected first stuck timestamp update to succeed");

        let repair = DefaultSelfRepair::new(Arc::clone(&cm), Duration::from_secs(60), 3);
        assert!(
            NativeSelfRepair::detect_stuck_jobs(&repair)
                .await
                .is_empty()
        );

        cm.update_context(job_id, |ctx| {
            let stuck_since = Utc::now() - chrono::Duration::seconds(120);
            let Some(last_transition) = ctx.transitions.last_mut() else {
                return Err("missing stuck transition".to_string());
            };
            last_transition.timestamp = stuck_since;
            Ok(())
        })
        .await
        .expect("failed to await update_context")
        .expect("expected second stuck timestamp update to succeed");

        let stuck_jobs = NativeSelfRepair::detect_stuck_jobs(&repair).await;
        assert_eq!(stuck_jobs.len(), 1);
        assert_eq!(
            stuck_jobs[0].last_activity,
            cm.get_context(job_id).await.unwrap().stuck_since().unwrap()
        );
        assert!(stuck_jobs[0].stuck_duration >= Duration::from_secs(60));
    }

    #[tokio::test]
    async fn repair_stuck_job_succeeds_within_limit() {
        let cm = Arc::new(ContextManager::new(10));
        let job_id = cm.create_job("Repairable", "desc").await.unwrap();

        // Move to InProgress -> Stuck.
        cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
            .await
            .unwrap()
            .unwrap();
        cm.update_context(job_id, |ctx| ctx.transition_to(JobState::Stuck, None))
            .await
            .unwrap()
            .unwrap();

        let repair = DefaultSelfRepair::new(Arc::clone(&cm), Duration::from_secs(60), 3);

        let stuck_job = StuckJob {
            job_id,
            last_activity: Utc::now(),
            stuck_duration: Duration::from_secs(120),
            last_error: None,
            repair_attempts: 0,
        };

        let result = NativeSelfRepair::repair_stuck_job(&repair, &stuck_job)
            .await
            .unwrap();
        assert!(
            matches!(result, RepairResult::Success { .. }),
            "Expected Success, got: {:?}",
            result
        );

        // Job should be back to InProgress after recovery.
        let ctx = cm.get_context(job_id).await.unwrap();
        assert_eq!(ctx.state, JobState::InProgress);
    }

    #[tokio::test]
    async fn repair_stuck_job_returns_manual_when_limit_exceeded() {
        let cm = Arc::new(ContextManager::new(10));
        let job_id = cm.create_job("Unrepairable", "desc").await.unwrap();

        let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 2);

        let stuck_job = StuckJob {
            job_id,
            last_activity: Utc::now(),
            stuck_duration: Duration::from_secs(300),
            last_error: Some("persistent failure".to_string()),
            repair_attempts: 2, // == max
        };

        let result = NativeSelfRepair::repair_stuck_job(&repair, &stuck_job)
            .await
            .unwrap();
        assert!(
            matches!(result, RepairResult::ManualRequired { .. }),
            "Expected ManualRequired, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn detect_broken_tools_returns_empty_without_store() {
        let cm = Arc::new(ContextManager::new(10));
        let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3);

        // No store configured, should return empty.
        let broken = NativeSelfRepair::detect_broken_tools(&repair).await;
        assert!(broken.is_empty());
    }

    #[tokio::test]
    async fn repair_broken_tool_returns_manual_without_builder() {
        let cm = Arc::new(ContextManager::new(10));
        let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3);

        let broken = BrokenTool {
            name: "test-tool".to_string(),
            failure_count: 10,
            last_error: Some("crash".to_string()),
            first_failure: Utc::now(),
            last_failure: Utc::now(),
            last_build_result: None,
            repair_attempts: 0,
        };

        let result = NativeSelfRepair::repair_broken_tool(&repair, &broken)
            .await
            .unwrap();
        assert!(
            matches!(result, RepairResult::ManualRequired { .. }),
            "Expected ManualRequired without builder, got: {:?}",
            result
        );
    }
}
