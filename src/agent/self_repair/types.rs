//! Domain types shared by the self-repair subsystem.

use core::{future::Future, pin::Pin};
use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A job that has been detected as stuck.
#[derive(Debug, Clone)]
pub struct StuckJob {
    pub job_id: Uuid,
    /// Timestamp when the job entered the `Stuck` state.
    pub stuck_since: DateTime<Utc>,
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
    pub message: String,
    pub route: RepairNotificationRoute,
}
