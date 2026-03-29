//! Background repair task orchestration.

use std::collections::HashSet;
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
    shutdown: &mut std::pin::Pin<&mut oneshot::Receiver<()>>,
    escalated_jobs: &mut HashSet<uuid::Uuid>,
) -> bool {
    let stuck_jobs = tokio::select! {
        biased;
        _ = shutdown.as_mut() => return false,
        stuck_jobs = repair.detect_stuck_jobs() => stuck_jobs,
    };
    let stuck_job_ids = stuck_jobs
        .iter()
        .map(|job| job.job_id)
        .collect::<HashSet<_>>();

    for job in stuck_jobs {
        match tokio::select! {
            biased;
            _ = shutdown.as_mut() => return false,
            result = repair.repair_stuck_job(&job) => result,
        } {
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
                );
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
                );
            }
            Ok(RepairResult::ManualRequired { message }) => {
                if escalated_jobs.insert(job.job_id) {
                    tracing::warn!(job = %job.job_id, status = "manual", "Stuck job repair requires manual intervention: {}", message);
                    send_notification(
                        notification_tx.as_mut(),
                        format!("Job {} needs manual intervention: {}", job.job_id, message),
                    );
                }
            }
            Err(e) => {
                tracing::error!(job = %job.job_id, "Stuck job repair error: {}", e);
            }
        }
    }

    escalated_jobs.retain(|job_id| stuck_job_ids.contains(job_id));

    true
}

async fn run_broken_tool_repairs(
    repair: &dyn SelfRepair,
    notification_tx: &mut Option<mpsc::Sender<RepairNotification>>,
    shutdown: &mut std::pin::Pin<&mut oneshot::Receiver<()>>,
    escalated_tools: &mut HashSet<String>,
) -> bool {
    let broken_tools = tokio::select! {
        biased;
        _ = shutdown.as_mut() => return false,
        broken_tools = repair.detect_broken_tools() => broken_tools,
    };
    let broken_tool_names = broken_tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<HashSet<_>>();

    for tool in broken_tools {
        match tokio::select! {
            biased;
            _ = shutdown.as_mut() => return false,
            result = repair.repair_broken_tool(&tool) => result,
        } {
            Ok(RepairResult::Success { message }) => {
                tracing::info!(
                    tool = %tool.name,
                    status = "success",
                    "Tool repair completed: {}",
                    message
                );
                send_notification(
                    notification_tx.as_mut(),
                    format!("Tool '{}' repaired: {}", tool.name, message),
                );
            }
            Ok(RepairResult::Failed { message }) => {
                tracing::error!(
                    tool = %tool.name,
                    status = "failed",
                    "Tool repair failed: {}",
                    message
                );
                send_notification(
                    notification_tx.as_mut(),
                    format!("Tool '{}' repair failed: {}", tool.name, message),
                );
            }
            Ok(RepairResult::ManualRequired { message }) => {
                if escalated_tools.insert(tool.name.clone()) {
                    tracing::warn!(
                        tool = %tool.name,
                        status = "manual",
                        "Tool repair requires manual intervention: {}",
                        message
                    );
                    send_notification(
                        notification_tx.as_mut(),
                        format!(
                            "Tool '{}' needs manual intervention: {}",
                            tool.name, message
                        ),
                    );
                }
            }
            Ok(RepairResult::Retry { message }) => {
                tracing::debug!(
                    tool = %tool.name,
                    status = "retry",
                    "Tool repair needs retry: {}",
                    message
                );
            }
            Err(e) => {
                tracing::error!(tool = %tool.name, "Tool repair error: {}", e);
            }
        }
    }

    escalated_tools.retain(|tool_name| broken_tool_names.contains(tool_name));

    true
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
        let mut escalated_jobs = HashSet::new();
        let mut escalated_tools = HashSet::new();

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::debug!("Repair task received shutdown signal");
                    break;
                }
                _ = tokio::time::sleep(check_interval) => {
                    if !run_stuck_job_repairs(&*repair, &mut notification_tx, &mut shutdown, &mut escalated_jobs).await {
                        tracing::debug!("Repair task received shutdown signal");
                        break;
                    }
                    if !run_broken_tool_repairs(
                        &*repair,
                        &mut notification_tx,
                        &mut shutdown,
                        &mut escalated_tools,
                    ).await {
                        tracing::debug!("Repair task received shutdown signal");
                        break;
                    }
                }
            }
        }
    }
}

fn send_notification(
    notification_tx: Option<&mut mpsc::Sender<RepairNotification>>,
    message: String,
) {
    if let Some(tx) = notification_tx {
        match tx.try_send(RepairNotification { message }) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!("Dropping repair notification because channel is full");
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                tracing::debug!("Dropping repair notification because receiver closed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
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

        shutdown_tx
            .send(())
            .expect("shutdown signal should send successfully");
        tokio::time::timeout(Duration::from_secs(1), task.run())
            .await
            .expect("repair task should stop promptly after shutdown");

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    struct BlockingSelfRepair {
        repair_started_tx: Mutex<Option<oneshot::Sender<()>>>,
    }

    impl NativeSelfRepair for BlockingSelfRepair {
        async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
            vec![StuckJob {
                job_id: uuid::Uuid::new_v4(),
                last_activity: chrono::Utc::now(),
                stuck_duration: Duration::from_secs(120),
                last_error: None,
                repair_attempts: 0,
            }]
        }

        async fn repair_stuck_job<'a>(
            &'a self,
            _job: &'a StuckJob,
        ) -> Result<RepairResult, RepairError> {
            if let Some(tx) = self
                .repair_started_tx
                .lock()
                .expect("repair-started sender lock should not be poisoned")
                .take()
            {
                tx.send(())
                    .expect("repair-started signal should send successfully");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
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
    async fn repair_task_stops_on_shutdown_during_active_cycle() {
        let (repair_started_tx, repair_started_rx) = oneshot::channel();
        let repair: Arc<dyn SelfRepair> = Arc::new(BlockingSelfRepair {
            repair_started_tx: Mutex::new(Some(repair_started_tx)),
        });
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = RepairTask::new(repair, Duration::from_millis(10), shutdown_rx);
        let handle = tokio::spawn(task.run());

        tokio::time::timeout(Duration::from_secs(1), repair_started_rx)
            .await
            .expect("repair cycle should start before shutdown")
            .expect("repair-started signal should be received");

        shutdown_tx
            .send(())
            .expect("shutdown signal should send successfully");
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("repair task should stop promptly during an active cycle")
            .expect("repair task should join cleanly");
    }
}
