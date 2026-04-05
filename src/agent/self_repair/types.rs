//! Domain types shared by the self-repair subsystem.

use core::{future::Future, pin::Pin};
use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A job that has been detected as stuck.
#[derive(Debug, Clone)]
pub struct StuckJob {
    /// Unique identifier of the stuck job.
    pub job_id: Uuid,
    /// Timestamp when the job entered the `Stuck` state.
    pub stuck_since: DateTime<Utc>,
    /// Duration the job has been stuck (calculated from stuck_since to detection time).
    pub stuck_duration: Duration,
    /// Optional error message from the last failure that caused the stuck state.
    pub last_error: Option<String>,
    /// Number of repair attempts already made for this stuck job.
    pub repair_attempts: u32,
}

/// A tool that has been detected as broken.
#[derive(Debug, Clone)]
pub struct BrokenTool {
    /// Name of the broken tool.
    pub name: String,
    /// Total number of failures recorded for this tool.
    pub failure_count: u32,
    /// Optional error message from the most recent failure.
    pub last_error: Option<String>,
    /// Timestamp of the first recorded failure.
    pub first_failure: DateTime<Utc>,
    /// Timestamp of the most recent failure.
    pub last_failure: DateTime<Utc>,
    /// Optional result from the last build attempt (for WASM tools).
    pub last_build_result: Option<serde_json::Value>,
    /// Number of repair attempts already made for this broken tool.
    pub repair_attempts: u32,
}

/// Result of a repair attempt.
#[derive(Debug, Clone)]
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

/// Destination metadata for a repair notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairNotificationRoute {
    /// Broadcast the notification to every registered channel for one user.
    BroadcastAll { user_id: String },
    /// Broadcast the notification to one proactive channel for one user.
    Broadcast { channel: String, user_id: String },
}

/// Notification emitted by the background repair loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairNotification {
    /// Human-readable message describing the repair event.
    pub message: String,
    /// Routing information for delivering the notification.
    pub route: RepairNotificationRoute,
}
