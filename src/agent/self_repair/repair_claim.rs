//! Axinite error and telemetry adaptation for the shared claim registry.

use uuid::Uuid;

use crate::agent::self_repair::BrokenTool;
use crate::agent::self_repair::claim_registry::{ClaimPoisoned, ClaimRegistry, RegistryClaim};
use crate::error::RepairError;

/// Tracks active same-tool repairs within one `DefaultSelfRepair` instance.
#[derive(Default)]
pub(super) struct RepairClaims {
    registry: ClaimRegistry,
}

impl RepairClaims {
    /// Claim a tool for repair, returning `None` when a same-tool repair is active.
    pub(super) fn claim_tool(
        &self,
        tool: &BrokenTool,
    ) -> Result<Option<ToolRepairClaim<'_>>, RepairError> {
        self.registry
            .claim(&tool.name, report_release)
            .map_err(|error| acquisition_error(&tool.name, error))
    }
}

/// The shared guard owns both the release transition and its RAII trigger.
pub(super) type ToolRepairClaim<'a> = RegistryClaim<'a>;

fn acquisition_error(tool_name: &str, error: ClaimPoisoned) -> RepairError {
    tracing::error!(
        tool_name = %tool_name,
        error = %error,
        "repair-claims mutex is poisoned; cannot acquire claim"
    );
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.repair.error", "category" => "claim_poisoned").increment(1);
    RepairError::Failed {
        target_type: "tool".to_string(),
        target_id: Uuid::nil(),
        reason: format!("failed to claim repair for {tool_name}: {error}"),
    }
}

fn report_release(tool_name: &str, result: Result<(), ClaimPoisoned>) {
    match result {
        Ok(()) => tracing::debug!(tool_name = %tool_name, "repair claim released"),
        Err(error) => tracing::error!(
            tool_name = %tool_name,
            error = %error,
            "repair-claims mutex is poisoned; cannot release claim"
        ),
    }
}

#[cfg(test)]
mod tests {
    //! Adapter regression checks for the real poisoned-registry path.

    use chrono::DateTime;

    use super::{BrokenTool, RepairClaims, RepairError, Uuid};
    use crate::agent::self_repair::claim_registry::tests::poison;

    #[test]
    #[tracing_test::traced_test]
    fn poisoned_registry_maps_to_tool_repair_error() {
        let claims = RepairClaims::default();
        poison(&claims.registry).expect_err("poison the adapter's actual registry");
        let tool = BrokenTool {
            name: "broken-tool".to_owned(),
            failure_count: 0,
            last_error: None,
            first_failure: DateTime::UNIX_EPOCH,
            last_failure: DateTime::UNIX_EPOCH,
            last_build_result: None,
            repair_attempts: 0,
        };
        match claims.claim_tool(&tool) {
            Err(RepairError::Failed {
                target_type,
                target_id,
                reason,
            }) => {
                assert_eq!(target_type, "tool");
                assert_eq!(target_id, Uuid::nil());
                assert_eq!(
                    reason,
                    concat!(
                        "failed to claim repair for broken-tool: ",
                        "poisoned lock: another task failed inside"
                    )
                );
            }
            _ => panic!("poisoned acquisition must map to RepairError::Failed"),
        }
        assert!(logs_contain(
            "repair-claims mutex is poisoned; cannot acquire claim"
        ));
        assert!(logs_contain("broken-tool"));
    }
}
