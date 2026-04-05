//! Shared type aliases and parameter-object re-exports for the database trait
//! surface.

use core::{future::Future, pin::Pin};

/// Boxed future used at dyn-backed database trait boundaries.
pub type DbFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub use crate::db::UserId;
pub use crate::db::traits::conversation::EnsureConversationParams;
pub use crate::db::traits::job::{EstimationActualsParams, EstimationSnapshotParams};
pub use crate::db::traits::routine::{RoutineRunCompletion, RoutineRuntimeUpdate};
pub use crate::db::traits::sandbox::{
    SandboxEventType, SandboxJobStatus, SandboxJobStatusUpdate, SandboxMode, SandboxModeParseError,
};
pub use crate::db::traits::settings::SettingKey;
pub use crate::db::traits::workspace::{HybridSearchParams, InsertChunkParams};

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{SandboxEventType, SandboxJobStatus, SandboxMode, SettingKey, UserId};

    #[rstest]
    #[case("", "")]
    #[case("user-123", "user-123")]
    fn user_id_conversions(#[case] input: &str, #[case] expected: &str) {
        let from_str = UserId::from(input);
        let from_string = UserId::from(input.to_string());

        assert_eq!(from_str.as_str(), expected);
        assert_eq!(from_string.as_str(), expected);
        assert_eq!(from_str.to_string(), expected);
        assert_eq!(from_string.to_string(), expected);
        assert_eq!(from_str.as_str(), from_str.to_string());
        assert_eq!(from_string.as_str(), from_string.to_string());
    }

    #[rstest]
    #[case("", "")]
    #[case("theme", "theme")]
    fn setting_key_conversions(#[case] input: &str, #[case] expected: &str) {
        let from_str = SettingKey::from(input);
        let from_string = SettingKey::from(input.to_string());

        assert_eq!(from_str.as_str(), expected);
        assert_eq!(from_string.as_str(), expected);
        assert_eq!(from_str.to_string(), expected);
        assert_eq!(from_string.to_string(), expected);
        assert_eq!(from_str.as_str(), from_str.to_string());
        assert_eq!(from_string.as_str(), from_string.to_string());
    }

    #[rstest]
    #[case("", "")]
    #[case("stdout", "stdout")]
    fn sandbox_event_type_conversions(#[case] input: &str, #[case] expected: &str) {
        let from_str = SandboxEventType::from(input);
        let from_string = SandboxEventType::from(input.to_string());

        assert_eq!(from_str.as_str(), expected);
        assert_eq!(from_string.as_str(), expected);
        assert_eq!(from_str.to_string(), expected);
        assert_eq!(from_string.to_string(), expected);
        assert_eq!(from_str.as_str(), from_str.to_string());
        assert_eq!(from_string.as_str(), from_string.to_string());
    }

    #[rstest]
    #[case("worker", Some(SandboxMode::Worker), None)]
    #[case("claude_code", Some(SandboxMode::ClaudeCode), None)]
    #[case("invalid", None, Some("unexpected sandbox mode 'invalid'"))]
    fn sandbox_mode_try_from(
        #[case] input: &str,
        #[case] expected_mode: Option<SandboxMode>,
        #[case] expected_error: Option<&str>,
    ) {
        let actual = SandboxMode::try_from(input);
        match (expected_mode, expected_error) {
            (Some(expected_mode), None) => assert_eq!(actual, Ok(expected_mode)),
            (None, Some(expected_error)) => assert_eq!(
                actual
                    .expect_err("invalid sandbox mode should fail")
                    .to_string(),
                expected_error
            ),
            _ => panic!("invalid test case"),
        }
    }

    #[rstest]
    #[case("", "")]
    #[case("creating", "creating")]
    fn sandbox_job_status_conversions(#[case] input: &str, #[case] expected: &str) {
        let from_str = SandboxJobStatus::from(input);
        let from_string = SandboxJobStatus::from(input.to_string());

        assert_eq!(from_str.as_str(), expected);
        assert_eq!(from_string.as_str(), expected);
        assert_eq!(from_str.to_string(), expected);
        assert_eq!(from_string.to_string(), expected);
    }
}
