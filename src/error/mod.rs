//! Error types for Axinite.

pub mod channel;
pub mod config;
pub mod database;
pub mod estimation;
pub mod evaluation;
pub mod job;
pub mod orchestrator;
pub mod repair;
pub mod routine;
pub mod safety;
pub mod tool;
pub mod worker;
pub mod workspace;

pub use self::channel::ChannelError;
pub use self::config::ConfigError;
pub use self::database::DatabaseError;
pub use self::estimation::EstimationError;
pub use self::evaluation::EvaluationError;
pub use self::job::JobError;
pub use self::orchestrator::OrchestratorError;
pub use self::repair::RepairError;
pub use self::routine::RoutineError;
pub use self::safety::SafetyError;
pub use self::tool::ToolError;
pub use self::worker::{ConfigMismatchField, WorkerError};
pub use self::workspace::WorkspaceError;
pub use crate::llm::error::LlmError;

/// Top-level error type for the agent.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Settings could not be loaded or validated.
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// A database operation failed.
    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),

    /// An input or output channel failed.
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),

    /// A call to an LLM provider failed.
    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),

    /// A tool could not be found, run, or returned an unusable result.
    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    /// The safety layer rejected or could not process content.
    #[error("Safety error: {0}")]
    Safety(#[from] SafetyError),

    /// A job could not be created, run, or completed.
    #[error("Job error: {0}")]
    Job(#[from] JobError),

    /// Cost or duration estimation failed.
    #[error("Estimation error: {0}")]
    Estimation(#[from] EstimationError),

    /// An evaluation step failed.
    #[error("Evaluation error: {0}")]
    Evaluation(#[from] EvaluationError),

    /// A self-repair attempt failed.
    #[error("Repair error: {0}")]
    Repair(#[from] RepairError),

    /// A workspace operation failed.
    #[error("Workspace error: {0}")]
    Workspace(#[from] WorkspaceError),

    /// A lifecycle hook failed.
    #[error("Hook error: {0}")]
    Hook(#[from] crate::hooks::HookError),

    /// Container management or the internal orchestrator API failed.
    #[error("Orchestrator error: {0}")]
    Orchestrator(#[from] OrchestratorError),

    /// A worker failed to execute its job.
    #[error("Worker error: {0}")]
    Worker(#[from] WorkerError),

    /// A scheduled routine failed.
    #[error("Routine error: {0}")]
    Routine(#[from] RoutineError),
}

/// Result type alias for the agent.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    //! Unit tests for error message content across error variants.

    use super::*;
    use rstest::rstest;
    use uuid::Uuid;

    #[rstest]
    #[case(
        Box::new(ConfigError::MissingEnvVar("DATABASE_URL".to_string())),
        &["DATABASE_URL"],
        &[]
    )]
    #[case(
        Box::new(ConfigError::MissingRequired {
            key: "llm.model".to_string(),
            hint: "Set LLM_MODEL env var".to_string(),
        }),
        &["llm.model", "Set LLM_MODEL"],
        &[]
    )]
    #[case(
        Box::new(ConfigError::InvalidValue {
            key: "port".to_string(),
            message: "must be a number".to_string(),
        }),
        &["port"],
        &[]
    )]
    #[case(
        Box::new(DatabaseError::NotFound {
            entity: "conversation".to_string(),
            id: "abc-123".to_string(),
        }),
        &["conversation", "abc-123"],
        &[]
    )]
    #[case(
        Box::new(DatabaseError::Query("syntax error near SELECT".to_string())),
        &["syntax error"],
        &[]
    )]
    #[case(
        Box::new(ChannelError::StartupFailed {
            name: "telegram".to_string(),
            reason: "invalid token".to_string(),
        }),
        &["telegram", "invalid token"],
        &[]
    )]
    #[case(
        Box::new(JobError::MaxJobsExceeded { max: 5 }),
        &["5"],
        &[]
    )]
    #[case(
        Box::new(JobError::NotFound { id: Uuid::nil() }),
        &["00000000-0000-0000-0000-000000000000"],
        &[]
    )]
    #[case(
        Box::new(SafetyError::InjectionDetected {
            pattern: "SYSTEM:".to_string(),
        }),
        &["SYSTEM:"],
        &[]
    )]
    #[case(
        Box::new(WorkspaceError::DocumentNotFound {
            doc_type: "notes".to_string(),
            user_id: "user1".to_string(),
        }),
        &["notes"],
        &["user1"]
    )]
    #[case(
        Box::new(RoutineError::InvalidCron {
            reason: "bad format".to_string(),
        }),
        &["bad format"],
        &[]
    )]
    fn error_display_contains_expected_text(
        #[case] err: Box<dyn std::error::Error>,
        #[case] expected_snippets: &[&str],
        #[case] unexpected_snippets: &[&str],
    ) {
        let msg = err.to_string();
        for snippet in expected_snippets {
            assert!(msg.contains(snippet), "expected '{snippet}' in: {msg}");
        }
        for snippet in unexpected_snippets {
            assert!(
                !msg.contains(snippet),
                "did not expect '{snippet}' in: {msg}"
            );
        }
    }

    enum ExpectedTopLevelVariant {
        Config,
        Database,
        Job,
        Safety,
    }

    #[rstest]
    #[case(
        || ConfigError::MissingEnvVar("TEST".to_string()).into(),
        ExpectedTopLevelVariant::Config
    )]
    #[case(
        || DatabaseError::Query("test".to_string()).into(),
        ExpectedTopLevelVariant::Database
    )]
    #[case(
        || JobError::MaxJobsExceeded { max: 1 }.into(),
        ExpectedTopLevelVariant::Job
    )]
    #[case(
        || SafetyError::ValidationFailed {
            reason: "test".to_string(),
        }
        .into(),
        ExpectedTopLevelVariant::Safety
    )]
    fn top_level_error_from_conversions(
        #[case] make_error: fn() -> Error,
        #[case] expected_top_level_variant: ExpectedTopLevelVariant,
    ) {
        let err = make_error();
        match expected_top_level_variant {
            ExpectedTopLevelVariant::Config => assert!(matches!(err, Error::Config(_))),
            ExpectedTopLevelVariant::Database => assert!(matches!(err, Error::Database(_))),
            ExpectedTopLevelVariant::Job => assert!(matches!(err, Error::Job(_))),
            ExpectedTopLevelVariant::Safety => assert!(matches!(err, Error::Safety(_))),
        }
    }
}
