//! Trait abstractions for self-repair implementations.

use core::future::Future;

use crate::error::RepairError;

pub use super::types::SelfRepairFuture;

use super::types::{BrokenTool, RepairResult, StuckJob};

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
