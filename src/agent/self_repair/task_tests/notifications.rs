//! Notification-focused tests for `RepairTask`.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rstest::rstest;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::super::{RepairNotification, RepairNotificationRoute, RepairTask};
use crate::agent::self_repair::{BrokenTool, NativeSelfRepair, RepairResult, SelfRepair, StuckJob};
use crate::error::RepairError;

/// Test-scoped mock for self-repair notifications.
#[derive(Clone)]
struct MockSelfRepair {
    stuck_jobs: Arc<tokio::sync::Mutex<Vec<StuckJob>>>,
    broken_tools: Arc<tokio::sync::Mutex<Vec<BrokenTool>>>,
    stuck_result: Arc<tokio::sync::Mutex<RepairResult>>,
    broken_result: Arc<tokio::sync::Mutex<RepairResult>>,
}

impl MockSelfRepair {
    fn new(
        stuck_jobs: Vec<StuckJob>,
        broken_tools: Vec<BrokenTool>,
        stuck_result: RepairResult,
        broken_result: RepairResult,
    ) -> Self {
        Self {
            stuck_jobs: Arc::new(tokio::sync::Mutex::new(stuck_jobs)),
            broken_tools: Arc::new(tokio::sync::Mutex::new(broken_tools)),
            stuck_result: Arc::new(tokio::sync::Mutex::new(stuck_result)),
            broken_result: Arc::new(tokio::sync::Mutex::new(broken_result)),
        }
    }
}

impl NativeSelfRepair for MockSelfRepair {
    async fn detect_stuck_jobs(&self) -> Vec<StuckJob> {
        self.stuck_jobs.lock().await.clone()
    }

    async fn repair_stuck_job<'a>(
        &'a self,
        _job: &'a StuckJob,
    ) -> Result<RepairResult, RepairError> {
        Ok(self.stuck_result.lock().await.clone())
    }

    async fn detect_broken_tools(&self) -> Vec<BrokenTool> {
        self.broken_tools.lock().await.clone()
    }

    async fn repair_broken_tool<'a>(
        &'a self,
        _tool: &'a BrokenTool,
    ) -> Result<RepairResult, RepairError> {
        Ok(self.broken_result.lock().await.clone())
    }
}

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

async fn assert_single_notification(
    mut harness: NotificationHarness,
    expected_message: &str,
    await_failure_msg: &str,
) {
    let notification = tokio::time::timeout(Duration::from_secs(1), harness.notification_rx.recv())
        .await
        .expect(await_failure_msg)
        .expect("notification channel should remain open");
    assert_eq!(notification.message, expected_message);
    harness.shutdown().await;
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

/// Test case parameters for repair notification tests.
#[derive(Debug, Clone)]
enum RepairTestCase {
    /// Stuck job with a given result type and expected message.
    StuckJob {
        job_id: Uuid,
        stuck_duration_secs: u64,
        result: RepairResult,
        expected_message: String,
    },
    /// Broken tool with a given result type and expected message.
    BrokenTool {
        name: String,
        result: RepairResult,
        expected_message: String,
    },
}

impl RepairTestCase {
    fn is_manual_required(&self) -> bool {
        matches!(
            self,
            RepairTestCase::StuckJob {
                result: RepairResult::ManualRequired { .. },
                ..
            } | RepairTestCase::BrokenTool {
                result: RepairResult::ManualRequired { .. },
                ..
            }
        )
    }
}

/// Create mock inputs for the given test case.
fn make_mock_inputs(
    case: &RepairTestCase,
) -> (Vec<StuckJob>, Vec<BrokenTool>, RepairResult, RepairResult) {
    match case.clone() {
        RepairTestCase::StuckJob {
            job_id,
            stuck_duration_secs,
            result,
            ..
        } => {
            let job = StuckJob {
                job_id,
                stuck_since: Utc::now(),
                stuck_duration: Duration::from_secs(stuck_duration_secs),
                last_error: None,
                repair_attempts: 0,
            };
            (
                vec![job],
                vec![],
                result,
                RepairResult::Success {
                    message: "noop".to_string(),
                },
            )
        }
        RepairTestCase::BrokenTool { name, result, .. } => {
            let tool = BrokenTool {
                name: name.clone(),
                failure_count: 5,
                last_error: Some("build failed".to_string()),
                first_failure: Utc::now(),
                last_failure: Utc::now(),
                last_build_result: None,
                repair_attempts: 0,
            };
            (
                vec![],
                vec![tool],
                RepairResult::Success {
                    message: "noop".to_string(),
                },
                result,
            )
        }
    }
}

#[rstest]
#[case::stuck_job_success(
    RepairTestCase::StuckJob {
        job_id: Uuid::nil(),
        stuck_duration_secs: 120,
        result: RepairResult::Success { message: "job recovered".to_string() },
        expected_message: "Job 00000000-0000-0000-0000-000000000000 was stuck for 120s, recovery succeeded: job recovered".to_string(),
    },
    "stuck-job success notification should arrive"
)]
#[case::broken_tool_success(
    RepairTestCase::BrokenTool {
        name: "compiler".to_string(),
        result: RepairResult::Success { message: "tool rebuilt".to_string() },
        expected_message: "Tool 'compiler' repaired: tool rebuilt".to_string(),
    },
    "broken-tool success notification should arrive"
)]
#[case::stuck_job_manual(
    RepairTestCase::StuckJob {
        job_id: Uuid::nil(),
        stuck_duration_secs: 120,
        result: RepairResult::ManualRequired { message: "manual job recovery".to_string() },
        expected_message: "Job 00000000-0000-0000-0000-000000000000 needs manual intervention: manual job recovery".to_string(),
    },
    "stuck-job manual notification should arrive"
)]
#[case::broken_tool_manual(
    RepairTestCase::BrokenTool {
        name: "compiler".to_string(),
        result: RepairResult::ManualRequired { message: "manual tool repair".to_string() },
        expected_message: "Tool 'compiler' needs manual intervention: manual tool repair".to_string(),
    },
    "broken-tool manual notification should arrive"
)]
#[case::stuck_job_failed(
    RepairTestCase::StuckJob {
        job_id: Uuid::nil(),
        stuck_duration_secs: 120,
        result: RepairResult::Failed { message: "recovery failed permanently".to_string() },
        expected_message: "Job 00000000-0000-0000-0000-000000000000 was stuck for 120s, recovery failed permanently: recovery failed permanently".to_string(),
    },
    "stuck-job failed notification should arrive"
)]
#[case::broken_tool_failed(
    RepairTestCase::BrokenTool {
        name: "compiler".to_string(),
        result: RepairResult::Failed { message: "rebuild failed".to_string() },
        expected_message: "Tool 'compiler' repair failed: rebuild failed".to_string(),
    },
    "broken-tool failed notification should arrive"
)]
#[tokio::test]
async fn repair_task_sends_notification(
    #[case] test_case: RepairTestCase,
    #[case] await_msg: &str,
) {
    let (stuck_jobs, broken_tools, stuck_result, broken_result) = make_mock_inputs(&test_case);
    let mock = MockSelfRepair::new(stuck_jobs, broken_tools, stuck_result, broken_result);
    // Blanket impl converts NativeSelfRepair -> SelfRepair
    let repair: Arc<dyn SelfRepair> = Arc::new(mock);
    let harness = spawn_notification_task(repair);

    let expected_message = match &test_case {
        RepairTestCase::StuckJob {
            expected_message, ..
        } => expected_message.clone(),
        RepairTestCase::BrokenTool {
            expected_message, ..
        } => expected_message.clone(),
    };

    if test_case.is_manual_required() {
        assert_manual_required_deduplication(
            harness,
            &expected_message,
            await_msg,
            "manual notification should be deduplicated",
        )
        .await;
    } else {
        assert_single_notification(harness, &expected_message, await_msg).await;
    }
}
