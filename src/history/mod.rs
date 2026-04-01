//! History and persistence layer.
//!
//! Stores job history, conversations, and actions in PostgreSQL for:
//! - Audit trail
//! - Learning from past executions
//! - Analytics and metrics

#[cfg(feature = "postgres")]
mod analytics;
#[cfg(feature = "postgres")]
pub(crate) mod migrations;
mod store;

#[cfg(feature = "postgres")]
pub use analytics::{JobStats, ToolStats};
#[cfg(feature = "postgres")]
pub(crate) use migrations::run_postgres_migrations;
#[cfg(feature = "postgres")]
pub use store::Store;
pub use store::{
    AgentJobRecord, AgentJobSummary, ConversationMessage, ConversationSummary, JobEventRecord,
    LlmCallRecord, SandboxJobRecord, SandboxJobSummary, SettingRow,
};
