//! Error types for IronClaw.

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
pub use self::worker::WorkerError;
pub use self::workspace::WorkspaceError;
pub use crate::llm::error::LlmError;

/// Top-level error type for the agent.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),

    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),

    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),

    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("Safety error: {0}")]
    Safety(#[from] SafetyError),

    #[error("Job error: {0}")]
    Job(#[from] JobError),

    #[error("Estimation error: {0}")]
    Estimation(#[from] EstimationError),

    #[error("Evaluation error: {0}")]
    Evaluation(#[from] EvaluationError),

    #[error("Repair error: {0}")]
    Repair(#[from] RepairError),

    #[error("Workspace error: {0}")]
    Workspace(#[from] WorkspaceError),

    #[error("Hook error: {0}")]
    Hook(#[from] crate::hooks::HookError),

    #[error("Orchestrator error: {0}")]
    Orchestrator(#[from] OrchestratorError),

    #[error("Worker error: {0}")]
    Worker(#[from] WorkerError),

    #[error("Routine error: {0}")]
    Routine(#[from] RoutineError),
}

/// Result type alias for the agent.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn config_error_display() {
        let err = ConfigError::MissingEnvVar("DATABASE_URL".to_string());
        let msg = err.to_string();
        assert!(
            msg.contains("DATABASE_URL"),
            "Should mention the variable name: {msg}"
        );

        let err = ConfigError::MissingRequired {
            key: "llm.model".to_string(),
            hint: "Set LLM_MODEL env var".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("llm.model"), "Should mention the key: {msg}");
        assert!(
            msg.contains("Set LLM_MODEL"),
            "Should include the hint: {msg}"
        );

        let err = ConfigError::InvalidValue {
            key: "port".to_string(),
            message: "must be a number".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("port"), "Should mention the key: {msg}");
    }

    #[test]
    fn database_error_display() {
        let err = DatabaseError::NotFound {
            entity: "conversation".to_string(),
            id: "abc-123".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("conversation"), "Should mention entity: {msg}");
        assert!(msg.contains("abc-123"), "Should mention id: {msg}");

        let err = DatabaseError::Query("syntax error near SELECT".to_string());
        assert!(err.to_string().contains("syntax error"));
    }

    #[test]
    fn channel_error_display() {
        let err = ChannelError::StartupFailed {
            name: "telegram".to_string(),
            reason: "invalid token".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("telegram"), "Should mention channel: {msg}");
        assert!(
            msg.contains("invalid token"),
            "Should mention reason: {msg}"
        );
    }

    #[test]
    fn job_error_display() {
        let err = JobError::MaxJobsExceeded { max: 5 };
        let msg = err.to_string();
        assert!(msg.contains("5"), "Should mention max: {msg}");

        let id = Uuid::new_v4();
        let err = JobError::NotFound { id };
        let msg = err.to_string();
        assert!(
            msg.contains(&id.to_string()),
            "Should mention job id: {msg}"
        );
    }

    #[test]
    fn safety_error_display() {
        let err = SafetyError::InjectionDetected {
            pattern: "SYSTEM:".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("SYSTEM:"), "Should mention pattern: {msg}");
    }

    #[test]
    fn workspace_error_display() {
        let err = WorkspaceError::DocumentNotFound {
            doc_type: "notes".to_string(),
            user_id: "user1".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("notes"), "Should mention doc_type: {msg}");
        assert!(msg.contains("user1"), "Should mention user_id: {msg}");
    }

    #[test]
    fn routine_error_display() {
        let err = RoutineError::InvalidCron {
            reason: "bad format".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("bad format"), "Should mention reason: {msg}");
    }

    #[test]
    fn top_level_error_from_conversions() {
        let config_err = ConfigError::MissingEnvVar("TEST".to_string());
        let err: Error = config_err.into();
        assert!(matches!(err, Error::Config(_)));

        let db_err = DatabaseError::Query("test".to_string());
        let err: Error = db_err.into();
        assert!(matches!(err, Error::Database(_)));

        let job_err = JobError::MaxJobsExceeded { max: 1 };
        let err: Error = job_err.into();
        assert!(matches!(err, Error::Job(_)));

        let safety_err = SafetyError::ValidationFailed {
            reason: "test".to_string(),
        };
        let err: Error = safety_err.into();
        assert!(matches!(err, Error::Safety(_)));
    }
}
