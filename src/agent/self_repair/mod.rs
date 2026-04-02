//! Self-repair orchestration for stuck jobs and broken tools.

use core::marker::PhantomData;

mod default;
mod task;
mod traits;
mod types;

pub use default::DefaultSelfRepair;
pub use task::RepairTask;
pub use traits::{NativeSelfRepair, SelfRepair, SelfRepairFuture};
pub use types::{BrokenTool, RepairNotification, RepairNotificationRoute, RepairResult, StuckJob};

struct NativeSelfRepairMarker<T: NativeSelfRepair>(PhantomData<T>);

const _: NativeSelfRepairMarker<DefaultSelfRepair> = NativeSelfRepairMarker(PhantomData);
const _: Option<SelfRepairFuture<'static, ()>> = None;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;
    use uuid::Uuid;

    use super::{BrokenTool, NativeSelfRepair, RepairResult, SelfRepair, StuckJob};
    use crate::error::RepairError;

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
            stuck_since: Utc::now(),
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
            repair
                .repair_stuck_job(&stuck_job)
                .await
                .expect("failed to get result from repair_stuck_job"),
            RepairResult::ManualRequired { .. }
        ));
        assert!(matches!(
            repair
                .repair_broken_tool(&broken_tool)
                .await
                .expect("failed to get result from repair_broken_tool"),
            RepairResult::ManualRequired { .. }
        ));
    }
}
