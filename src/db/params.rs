//! Shared type aliases and parameter-object re-exports for the database trait
//! surface.

use core::{future::Future, pin::Pin};

/// Boxed future used at dyn-backed database trait boundaries.
pub type DbFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub use crate::db::traits::conversation::EnsureConversationParams;
pub use crate::db::traits::job::{EstimationActualsParams, EstimationSnapshotParams};
pub use crate::db::traits::routine::{RoutineRunCompletion, RoutineRuntimeUpdate};
pub use crate::db::traits::sandbox::{SandboxEventType, SandboxJobStatusUpdate, SandboxMode};
pub use crate::db::traits::settings::{SettingKey, UserId};
pub use crate::db::traits::workspace::{HybridSearchParams, InsertChunkParams};

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{SandboxEventType, SandboxMode, SettingKey, UserId};

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
    #[case("worker", Ok(SandboxMode::Worker))]
    #[case("claude_code", Ok(SandboxMode::ClaudeCode))]
    #[case(
        "invalid",
        Err("unexpected sandbox mode 'invalid'".to_string())
    )]
    fn sandbox_mode_try_from(#[case] input: &str, #[case] expected: Result<SandboxMode, String>) {
        let actual = SandboxMode::try_from(input);

        assert_eq!(actual, expected);
    }
}
