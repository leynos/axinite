//! Notification-focused tests for `RepairTask`.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::super::{RepairNotification, RepairTask};
use crate::agent::self_repair::{BrokenTool, NativeSelfRepair, RepairResult, SelfRepair, StuckJob};
use crate::error::RepairError;

struct NotificationHarness {
    notification_rx: mpsc::Receiver<RepairNotification>,
    shutdown_tx: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

impl NotificationHarness {
    async fn shutdown(self) {
        self.shutdown_tx
            .send(())
            .expect("shutdown signal should send successfully");
        tokio::time::timeout(Duration::from_secs(1), self.handle)
            .await
            .expect("repair task should stop promptly")
            .expect("repair task should join cleanly");
    }
}

fn spawn_notification_task(repair: Arc<dyn SelfRepair>) -> NotificationHarness {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (notification_tx, notification_rx) = mpsc::channel(8);
    let task = RepairTask::new(repair, Duration::from_millis(10), shutdown_rx)
        .with_notification_tx(notification_tx);
    let handle = tokio::spawn(task.run());

    NotificationHarness {
        notification_rx,
        shutdown_tx,
        handle,
    }
}

struct StuckSuccessSelfRepair;

impl NativeSelfRepair for StuckSuccessSelfRepair {
    async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
        vec![StuckJob {
            job_id: Uuid::new_v4(),
            last_activity: Utc::now(),
            stuck_duration: Duration::from_secs(120),
            last_error: None,
            repair_attempts: 0,
        }]
    }

    async fn repair_stuck_job<'a>(
        &'a self,
        _job: &'a StuckJob,
    ) -> Result<RepairResult, RepairError> {
        Ok(RepairResult::Success {
            message: "job recovered".to_string(),
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
async fn repair_task_sends_notification_for_stuck_job_success() {
    let repair: Arc<dyn SelfRepair> = Arc::new(StuckSuccessSelfRepair);
    let mut harness = spawn_notification_task(repair);

    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect("stuck-job success notification should arrive")
        .expect("notification channel should remain open");

    assert!(notification.message.contains("recovery succeeded"));

    harness.shutdown().await;
}

struct BrokenToolSuccessSelfRepair;

impl NativeSelfRepair for BrokenToolSuccessSelfRepair {
    async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
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
        vec![BrokenTool {
            name: "compiler".to_string(),
            failure_count: 5,
            last_error: Some("build failed".to_string()),
            first_failure: Utc::now(),
            last_failure: Utc::now(),
            last_build_result: None,
            repair_attempts: 0,
        }]
    }

    async fn repair_broken_tool<'a>(
        &'a self,
        _tool: &'a BrokenTool,
    ) -> Result<RepairResult, RepairError> {
        Ok(RepairResult::Success {
            message: "tool rebuilt".to_string(),
        })
    }
}

#[tokio::test]
async fn repair_task_sends_notification_for_broken_tool_success() {
    let repair: Arc<dyn SelfRepair> = Arc::new(BrokenToolSuccessSelfRepair);
    let mut harness = spawn_notification_task(repair);

    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect("broken-tool success notification should arrive")
        .expect("notification channel should remain open");

    assert_eq!(
        notification.message,
        "Tool 'compiler' repaired: tool rebuilt"
    );

    harness.shutdown().await;
}

struct StuckManualSelfRepair;

impl NativeSelfRepair for StuckManualSelfRepair {
    async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
        vec![StuckJob {
            job_id: Uuid::nil(),
            last_activity: Utc::now(),
            stuck_duration: Duration::from_secs(120),
            last_error: None,
            repair_attempts: 3,
        }]
    }

    async fn repair_stuck_job<'a>(
        &'a self,
        _job: &'a StuckJob,
    ) -> Result<RepairResult, RepairError> {
        Ok(RepairResult::ManualRequired {
            message: "manual job recovery".to_string(),
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
async fn repair_task_deduplicates_manual_required_notifications_for_stuck_jobs() {
    let repair: Arc<dyn SelfRepair> = Arc::new(StuckManualSelfRepair);
    let mut harness = spawn_notification_task(repair);

    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect("stuck-job manual notification should arrive")
        .expect("notification channel should remain open");
    assert_eq!(
        notification.message,
        format!(
            "Job {} needs manual intervention: manual job recovery",
            Uuid::nil()
        )
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        matches!(
            harness.notification_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "manual stuck-job notification should be deduplicated"
    );

    harness.shutdown().await;
}

struct BrokenToolManualSelfRepair;

impl NativeSelfRepair for BrokenToolManualSelfRepair {
    async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
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
        vec![BrokenTool {
            name: "compiler".to_string(),
            failure_count: 5,
            last_error: Some("build failed".to_string()),
            first_failure: Utc::now(),
            last_failure: Utc::now(),
            last_build_result: None,
            repair_attempts: 3,
        }]
    }

    async fn repair_broken_tool<'a>(
        &'a self,
        _tool: &'a BrokenTool,
    ) -> Result<RepairResult, RepairError> {
        Ok(RepairResult::ManualRequired {
            message: "manual tool repair".to_string(),
        })
    }
}

#[tokio::test]
async fn repair_task_deduplicates_manual_required_notifications_for_broken_tools() {
    let repair: Arc<dyn SelfRepair> = Arc::new(BrokenToolManualSelfRepair);
    let mut harness = spawn_notification_task(repair);

    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect("broken-tool manual notification should arrive")
        .expect("notification channel should remain open");
    assert_eq!(
        notification.message,
        "Tool 'compiler' needs manual intervention: manual tool repair"
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        matches!(
            harness.notification_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "manual broken-tool notification should be deduplicated"
    );

    harness.shutdown().await;
}
