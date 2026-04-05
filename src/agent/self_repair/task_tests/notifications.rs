//! Notification-focused tests for `RepairTask`.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::super::{RepairNotification, RepairNotificationRoute, RepairTask};
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
        .with_notification_tx(
            notification_tx,
            RepairNotificationRoute::BroadcastAll {
                user_id: "default".to_string(),
            },
        );
    let handle = tokio::spawn(task.run());

    NotificationHarness {
        notification_rx,
        shutdown_tx,
        handle,
    }
}

async fn assert_manual_required_deduplication(
    mut harness: NotificationHarness,
    expected_message: &str,
    await_failure_msg: &str,
    dedup_failure_msg: &str,
) {
    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect(await_failure_msg)
        .expect("notification channel should remain open");
    assert_eq!(notification.message, expected_message);

    assert!(
        tokio::time::timeout(Duration::from_millis(50), harness.notification_rx.recv())
            .await
            .is_err(),
        "{dedup_failure_msg}"
    );

    harness.shutdown().await;
}

struct MockSelfRepair {
    stuck_jobs: Vec<StuckJob>,
    broken_tools: Vec<BrokenTool>,
    stuck_repair_result: RepairResult,
    broken_repair_result: RepairResult,
}

impl MockSelfRepair {
    fn with_stuck_job(job: StuckJob, result: RepairResult) -> Self {
        Self {
            stuck_jobs: vec![job],
            broken_tools: vec![],
            stuck_repair_result: result,
            broken_repair_result: RepairResult::Success {
                message: "noop".to_string(),
            },
        }
    }

    fn with_broken_tool(tool: BrokenTool, result: RepairResult) -> Self {
        Self {
            stuck_jobs: vec![],
            broken_tools: vec![tool],
            stuck_repair_result: RepairResult::Success {
                message: "noop".to_string(),
            },
            broken_repair_result: result,
        }
    }
}

impl NativeSelfRepair for MockSelfRepair {
    async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
        self.stuck_jobs.clone()
    }

    async fn repair_stuck_job<'a>(
        &'a self,
        _job: &'a StuckJob,
    ) -> Result<RepairResult, RepairError> {
        Ok(self.stuck_repair_result.clone())
    }

    async fn detect_broken_tools(&self) -> Vec<BrokenTool> {
        self.broken_tools.clone()
    }

    async fn repair_broken_tool<'a>(
        &'a self,
        _tool: &'a BrokenTool,
    ) -> Result<RepairResult, RepairError> {
        Ok(self.broken_repair_result.clone())
    }
}

#[tokio::test]
async fn repair_task_sends_notification_for_stuck_job_success() {
    let repair: Arc<dyn SelfRepair> = Arc::new(MockSelfRepair::with_stuck_job(
        StuckJob {
            job_id: Uuid::new_v4(),
            stuck_since: Utc::now(),
            stuck_duration: Duration::from_secs(120),
            last_error: None,
            repair_attempts: 0,
        },
        RepairResult::Success {
            message: "job recovered".to_string(),
        },
    ));
    let mut harness = spawn_notification_task(repair);

    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect("stuck-job success notification should arrive")
        .expect("notification channel should remain open");

    assert!(notification.message.contains("recovery succeeded"));

    harness.shutdown().await;
}

#[tokio::test]
async fn repair_task_sends_notification_for_broken_tool_success() {
    let repair: Arc<dyn SelfRepair> = Arc::new(MockSelfRepair::with_broken_tool(
        BrokenTool {
            name: "compiler".to_string(),
            failure_count: 5,
            last_error: Some("build failed".to_string()),
            first_failure: Utc::now(),
            last_failure: Utc::now(),
            last_build_result: None,
            repair_attempts: 0,
        },
        RepairResult::Success {
            message: "tool rebuilt".to_string(),
        },
    ));
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

#[tokio::test]
async fn repair_task_deduplicates_manual_required_notifications_for_stuck_jobs() {
    let repair: Arc<dyn SelfRepair> = Arc::new(MockSelfRepair::with_stuck_job(
        StuckJob {
            job_id: Uuid::nil(),
            stuck_since: Utc::now(),
            stuck_duration: Duration::from_secs(120),
            last_error: None,
            repair_attempts: 3,
        },
        RepairResult::ManualRequired {
            message: "manual job recovery".to_string(),
        },
    ));
    let harness = spawn_notification_task(repair);
    assert_manual_required_deduplication(
        harness,
        &format!(
            "Job {} needs manual intervention: manual job recovery",
            Uuid::nil()
        ),
        "stuck-job manual notification should arrive",
        "manual stuck-job notification should be deduplicated",
    )
    .await;
}

#[tokio::test]
async fn repair_task_deduplicates_manual_required_notifications_for_broken_tools() {
    let repair: Arc<dyn SelfRepair> = Arc::new(MockSelfRepair::with_broken_tool(
        BrokenTool {
            name: "compiler".to_string(),
            failure_count: 5,
            last_error: Some("build failed".to_string()),
            first_failure: Utc::now(),
            last_failure: Utc::now(),
            last_build_result: None,
            repair_attempts: 3,
        },
        RepairResult::ManualRequired {
            message: "manual tool repair".to_string(),
        },
    ));
    let harness = spawn_notification_task(repair);
    assert_manual_required_deduplication(
        harness,
        "Tool 'compiler' needs manual intervention: manual tool repair",
        "broken-tool manual notification should arrive",
        "manual broken-tool notification should be deduplicated",
    )
    .await;
}

#[tokio::test]
async fn repair_task_sends_notification_for_stuck_job_failed() {
    let repair: Arc<dyn SelfRepair> = Arc::new(MockSelfRepair::with_stuck_job(
        StuckJob {
            job_id: Uuid::nil(),
            stuck_since: Utc::now(),
            stuck_duration: Duration::from_secs(120),
            last_error: Some("persistent failure".to_string()),
            repair_attempts: 2,
        },
        RepairResult::Failed {
            message: "recovery failed permanently".to_string(),
        },
    ));
    let mut harness = spawn_notification_task(repair);

    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect("stuck-job failed notification should arrive")
        .expect("notification channel should remain open");

    assert_eq!(
        notification.message,
        format!(
            "Job {} was stuck for {}s, recovery failed permanently: recovery failed permanently",
            Uuid::nil(),
            120
        )
    );

    harness.shutdown().await;
}

#[tokio::test]
async fn repair_task_sends_notification_for_broken_tool_failed() {
    let repair: Arc<dyn SelfRepair> = Arc::new(MockSelfRepair::with_broken_tool(
        BrokenTool {
            name: "compiler".to_string(),
            failure_count: 10,
            last_error: Some("build failed".to_string()),
            first_failure: Utc::now(),
            last_failure: Utc::now(),
            last_build_result: None,
            repair_attempts: 2,
        },
        RepairResult::Failed {
            message: "rebuild failed".to_string(),
        },
    ));
    let mut harness = spawn_notification_task(repair);

    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect("broken-tool failed notification should arrive")
        .expect("notification channel should remain open");

    assert_eq!(
        notification.message,
        "Tool 'compiler' repair failed: rebuild failed"
    );

    harness.shutdown().await;
}
