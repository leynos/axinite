//! Shutdown-focused tests for `RepairTask`.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::oneshot;

use super::super::RepairTask;
use crate::agent::self_repair::{BrokenTool, NativeSelfRepair, RepairResult, SelfRepair, StuckJob};
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
