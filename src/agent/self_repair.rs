//! Self-repair for stuck jobs and broken tools.

use core::future::Future;
use core::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::context::{ContextManager, JobState};
use crate::db::Database;
use crate::error::RepairError;
use crate::tools::builder::ProjectName;
use crate::tools::{BuildRequirement, Language, SoftwareBuilder, SoftwareType, ToolRegistry};

/// A job that has been detected as stuck.
#[derive(Debug, Clone)]
pub struct StuckJob {
    pub job_id: Uuid,
    pub last_activity: DateTime<Utc>,
    pub stuck_duration: Duration,
    pub last_error: Option<String>,
    pub repair_attempts: u32,
}

/// A tool that has been detected as broken.
#[derive(Debug, Clone)]
pub struct BrokenTool {
    pub name: String,
    pub failure_count: u32,
    pub last_error: Option<String>,
    pub first_failure: DateTime<Utc>,
    pub last_failure: DateTime<Utc>,
    pub last_build_result: Option<serde_json::Value>,
    pub repair_attempts: u32,
}

/// Result of a repair attempt.
#[derive(Debug)]
pub enum RepairResult {
    /// Repair was successful.
    Success { message: String },
    /// Repair failed but can be retried.
    Retry { message: String },
    /// Repair failed permanently.
    Failed { message: String },
    /// Manual intervention required.
    ManualRequired { message: String },
}

/// Boxed future used at the dyn self-repair boundary.
pub type SelfRepairFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for self-repair implementations.
pub trait SelfRepair: Send + Sync {
    /// Detect stuck jobs.
    fn detect_stuck_jobs(&self) -> SelfRepairFuture<'_, Vec<StuckJob>>;

    /// Attempt to repair a stuck job.
    fn repair_stuck_job<'a>(
        &'a self,
        job: &'a StuckJob,
    ) -> SelfRepairFuture<'a, Result<RepairResult, RepairError>>;

    /// Detect broken tools.
    fn detect_broken_tools(&self) -> SelfRepairFuture<'_, Vec<BrokenTool>>;

    /// Attempt to repair a broken tool.
    fn repair_broken_tool<'a>(
        &'a self,
        tool: &'a BrokenTool,
    ) -> SelfRepairFuture<'a, Result<RepairResult, RepairError>>;
}

/// Native async sibling trait for concrete self-repair implementations.
pub trait NativeSelfRepair: Send + Sync {
    /// Detect stuck jobs.
    fn detect_stuck_jobs(&self) -> impl Future<Output = Vec<StuckJob>> + Send + '_;

    /// Attempt to repair a stuck job.
    fn repair_stuck_job<'a>(
        &'a self,
        job: &'a StuckJob,
    ) -> impl Future<Output = Result<RepairResult, RepairError>> + Send + 'a;

    /// Detect broken tools.
    fn detect_broken_tools(&self) -> impl Future<Output = Vec<BrokenTool>> + Send + '_;

    /// Attempt to repair a broken tool.
    fn repair_broken_tool<'a>(
        &'a self,
        tool: &'a BrokenTool,
    ) -> impl Future<Output = Result<RepairResult, RepairError>> + Send + 'a;
}

impl<T> SelfRepair for T
where
    T: NativeSelfRepair + Send + Sync,
{
    fn detect_stuck_jobs(&self) -> SelfRepairFuture<'_, Vec<StuckJob>> {
        Box::pin(async move { NativeSelfRepair::detect_stuck_jobs(self).await })
    }

    fn repair_stuck_job<'a>(
        &'a self,
        job: &'a StuckJob,
    ) -> SelfRepairFuture<'a, Result<RepairResult, RepairError>> {
        Box::pin(async move { NativeSelfRepair::repair_stuck_job(self, job).await })
    }

    fn detect_broken_tools(&self) -> SelfRepairFuture<'_, Vec<BrokenTool>> {
        Box::pin(async move { NativeSelfRepair::detect_broken_tools(self).await })
    }

    fn repair_broken_tool<'a>(
        &'a self,
        tool: &'a BrokenTool,
    ) -> SelfRepairFuture<'a, Result<RepairResult, RepairError>> {
        Box::pin(async move { NativeSelfRepair::repair_broken_tool(self, tool).await })
    }
}

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
        let stuck_ids = self.context_manager.find_stuck_jobs().await;
        let mut stuck_jobs = Vec::new();
        let now = Utc::now();

        for job_id in stuck_ids {
            if let Ok(ctx) = self.context_manager.get_context(job_id).await
                && ctx.state == JobState::Stuck
                && let Some(stuck_since) = ctx.stuck_since()
            {
                let stuck_duration = duration_since(now, stuck_since);
                if stuck_duration < self.stuck_threshold {
                    continue;
                }

                stuck_jobs.push(StuckJob {
                    job_id,
                    last_activity: stuck_since,
                    stuck_duration,
                    last_error: None,
                    repair_attempts: ctx.repair_attempts,
                });
            }
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

/// Notification emitted by the background repair loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairNotification {
    pub message: String,
}

/// Background repair task that periodically checks for and repairs issues.
pub struct RepairTask {
    repair: Arc<dyn SelfRepair>,
    check_interval: Duration,
    shutdown_rx: oneshot::Receiver<()>,
    notification_tx: Option<mpsc::Sender<RepairNotification>>,
}

impl RepairTask {
    /// Create a new repair task.
    pub fn new(
        repair: Arc<dyn SelfRepair>,
        check_interval: Duration,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> Self {
        Self {
            repair,
            check_interval,
            shutdown_rx,
            notification_tx: None,
        }
    }

    /// Forward noteworthy repair outcomes to an external observer.
    pub fn with_notification_tx(
        mut self,
        notification_tx: mpsc::Sender<RepairNotification>,
    ) -> Self {
        self.notification_tx = Some(notification_tx);
        self
    }

    /// Run the repair task.
    pub async fn run(self) {
        let Self {
            repair,
            check_interval,
            shutdown_rx,
            mut notification_tx,
        } = self;
        let mut shutdown = std::pin::pin!(shutdown_rx);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::debug!("Repair task received shutdown signal");
                    break;
                }
                _ = tokio::time::sleep(check_interval) => {
                    // Check for stuck jobs
                    let stuck_jobs = repair.detect_stuck_jobs().await;
                    for job in stuck_jobs {
                        match repair.repair_stuck_job(&job).await {
                            Ok(RepairResult::Success { message }) => {
                                tracing::info!(job = %job.job_id, status = "success", "Stuck job repair completed: {}", message);
                                send_notification(
                                    notification_tx.as_mut(),
                                    format!(
                                        "Job {} was stuck for {}s, recovery succeeded: {}",
                                        job.job_id,
                                        job.stuck_duration.as_secs(),
                                        message
                                    ),
                                )
                                .await;
                            }
                            Ok(RepairResult::Retry { message }) => {
                                tracing::debug!(job = %job.job_id, status = "retry", "Stuck job repair needs retry: {}", message);
                            }
                            Ok(RepairResult::Failed { message }) => {
                                tracing::error!(job = %job.job_id, status = "failed", "Stuck job repair failed: {}", message);
                                send_notification(
                                    notification_tx.as_mut(),
                                    format!(
                                        "Job {} was stuck for {}s, recovery failed permanently: {}",
                                        job.job_id,
                                        job.stuck_duration.as_secs(),
                                        message
                                    ),
                                )
                                .await;
                            }
                            Ok(RepairResult::ManualRequired { message }) => {
                                tracing::warn!(job = %job.job_id, status = "manual", "Stuck job repair requires manual intervention: {}", message);
                                send_notification(
                                    notification_tx.as_mut(),
                                    format!("Job {} needs manual intervention: {}", job.job_id, message),
                                )
                                .await;
                            }
                            Err(e) => {
                                tracing::error!(job = %job.job_id, "Stuck job repair error: {}", e);
                            }
                        }
                    }

                    // Check for broken tools
                    let broken_tools = repair.detect_broken_tools().await;
                    for tool in broken_tools {
                        match repair.repair_broken_tool(&tool).await {
                            Ok(RepairResult::Success { message }) => {
                                tracing::debug!(tool = %tool.name, status = "completed", "Tool repair completed: {:?}", message);
                                send_notification(
                                    notification_tx.as_mut(),
                                    format!("Tool '{}' repaired: {}", tool.name, message),
                                )
                                .await;
                            }
                            Ok(result) => {
                                tracing::debug!(tool = %tool.name, status = "completed", "Tool repair completed: {:?}", result);
                            }
                            Err(e) => {
                                tracing::error!(tool = %tool.name, "Tool repair error: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn send_notification(
    notification_tx: Option<&mut mpsc::Sender<RepairNotification>>,
    message: String,
) {
    if let Some(tx) = notification_tx
        && let Err(error) = tx.send(RepairNotification { message }).await
    {
        tracing::debug!("Dropping repair notification because receiver closed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn test_repair_result_variants() {
        let success = RepairResult::Success {
            message: "OK".to_string(),
        };
        assert!(matches!(success, RepairResult::Success { .. }));

        let manual = RepairResult::ManualRequired {
            message: "Help needed".to_string(),
        };
        assert!(matches!(manual, RepairResult::ManualRequired { .. }));
    }

    struct PassiveSelfRepair;

    impl NativeSelfRepair for PassiveSelfRepair {
        async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
            vec![]
        }

        async fn repair_stuck_job<'a>(
            &'a self,
            _job: &'a StuckJob,
        ) -> Result<RepairResult, RepairError> {
            Ok(RepairResult::ManualRequired {
                message: "noop".to_string(),
            })
        }

        async fn detect_broken_tools(&self) -> Vec<BrokenTool> {
            vec![]
        }

        async fn repair_broken_tool<'a>(
            &'a self,
            _tool: &'a BrokenTool,
        ) -> Result<RepairResult, RepairError> {
            Ok(RepairResult::ManualRequired {
                message: "noop".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn self_repair_blanket_adapter_uses_native_trait() {
        let repair: Arc<dyn SelfRepair> = Arc::new(PassiveSelfRepair);

        assert!(repair.detect_stuck_jobs().await.is_empty());
        assert!(repair.detect_broken_tools().await.is_empty());

        let stuck_job = StuckJob {
            job_id: Uuid::new_v4(),
            last_activity: Utc::now(),
            stuck_duration: Duration::from_secs(120),
            last_error: Some("stalled".to_string()),
            repair_attempts: 1,
        };
        let broken_tool = BrokenTool {
            name: "demo-tool".to_string(),
            failure_count: 3,
            last_error: Some("boom".to_string()),
            first_failure: Utc::now(),
            last_failure: Utc::now(),
            last_build_result: None,
            repair_attempts: 1,
        };

        assert!(matches!(
            repair.repair_stuck_job(&stuck_job).await.unwrap(),
            RepairResult::ManualRequired { .. }
        ));
        assert!(matches!(
            repair.repair_broken_tool(&broken_tool).await.unwrap(),
            RepairResult::ManualRequired { .. }
        ));
    }

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
        let job_id = cm.create_job("Stuck job", "desc").await.unwrap();

        cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
            .await
            .unwrap()
            .unwrap();
        cm.update_context(job_id, |ctx| ctx.transition_to(JobState::Stuck, None))
            .await
            .unwrap()
            .unwrap();
        cm.update_context(job_id, |ctx| {
            let stuck_since = Utc::now() - chrono::Duration::seconds(30);
            let Some(last_transition) = ctx.transitions.last_mut() else {
                return Err("missing stuck transition".to_string());
            };
            last_transition.timestamp = stuck_since;
            Ok(())
        })
        .await
        .unwrap()
        .unwrap();

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
        .unwrap()
        .unwrap();

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

    struct CountingSelfRepair {
        detect_stuck_jobs_calls: Arc<AtomicUsize>,
    }

    impl NativeSelfRepair for CountingSelfRepair {
        async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
            self.detect_stuck_jobs_calls.fetch_add(1, Ordering::SeqCst);
            vec![]
        }

        async fn repair_stuck_job<'a>(
            &'a self,
            _job: &'a StuckJob,
        ) -> Result<RepairResult, RepairError> {
            Ok(RepairResult::Success {
                message: "noop".to_string(),
            })
        }

        async fn detect_broken_tools(&self) -> Vec<BrokenTool> {
            vec![]
        }

        async fn repair_broken_tool<'a>(
            &'a self,
            _tool: &'a BrokenTool,
        ) -> Result<RepairResult, RepairError> {
            Ok(RepairResult::Success {
                message: "noop".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn repair_task_stops_on_shutdown_before_running_a_cycle() {
        let calls = Arc::new(AtomicUsize::new(0));
        let repair: Arc<dyn SelfRepair> = Arc::new(CountingSelfRepair {
            detect_stuck_jobs_calls: Arc::clone(&calls),
        });
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = RepairTask::new(repair, Duration::from_secs(60), shutdown_rx);

        shutdown_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), task.run())
            .await
            .expect("repair task should stop promptly after shutdown");

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
