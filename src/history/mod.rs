//! History and persistence layer.
//!
//! Stores job history, conversations, and actions in PostgreSQL for:
//! - Audit trail
//! - Learning from past executions
//! - Analytics and metrics

#[cfg(feature = "postgres")]
mod analytics;
mod store;

#[cfg(feature = "postgres")]
pub use analytics::{JobStats, ToolStats};
#[cfg(feature = "postgres")]
pub use store::Store;
#[cfg(feature = "postgres")]
pub(crate) use store::repair_postgres_refinery_history;
pub use store::{
    AgentJobRecord, AgentJobSummary, ConversationMessage, ConversationSummary, JobEventRecord,
    LlmCallRecord, SandboxJobRecord, SandboxJobSummary, SettingRow,
};
