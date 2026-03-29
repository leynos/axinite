//! Background repair task orchestration.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use super::traits::SelfRepair;
use super::types::{RepairNotification, RepairResult};

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
}

async fn run_stuck_job_repairs(
    repair: &dyn SelfRepair,
    notification_tx: &mut Option<mpsc::Sender<RepairNotification>>,
) {
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
}

async fn run_broken_tool_repairs(
    repair: &dyn SelfRepair,
    notification_tx: &mut Option<mpsc::Sender<RepairNotification>>,
) {
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

impl RepairTask {
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
                    run_stuck_job_repairs(&*repair, &mut notification_tx).await;
                    run_broken_tool_repairs(&*repair, &mut notification_tx).await;
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::sync::oneshot;

    use super::RepairTask;
    use crate::agent::self_repair::{
        BrokenTool, NativeSelfRepair, RepairResult, SelfRepair, StuckJob,
    };
    use crate::error::RepairError;

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
