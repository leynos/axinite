//! In-process repair claim tracking for self-repair.

use std::collections::HashSet;
use std::sync::Mutex;

use uuid::Uuid;

use crate::agent::self_repair::BrokenTool;
use crate::error::RepairError;

/// Tracks active same-tool repairs within one `DefaultSelfRepair` instance.
#[derive(Default)]
pub(super) struct RepairClaims {
    active_tool_repairs: Mutex<HashSet<String>>,
}

impl RepairClaims {
    /// Claim a tool for repair, returning `None` when a same-tool repair is active.
    pub(super) fn claim_tool(
        &self,
        tool: &BrokenTool,
    ) -> Result<Option<ToolRepairClaim<'_>>, RepairError> {
        let mut active_repairs = match self.active_tool_repairs.lock() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::error!(
                    tool_name = %tool.name,
                    error = %e,
                    "repair-claims mutex is poisoned; cannot acquire claim"
                );
                #[cfg(feature = "metrics")]
                metrics::counter!(
                    "axinite.repair.error",
                    "category" => "claim_poisoned"
                )
                .increment(1);
                return Err(RepairError::Failed {
                    target_type: "tool".to_string(),
                    target_id: Uuid::nil(),
                    reason: format!("failed to claim repair for {}: {}", tool.name, e),
                });
            }
        };

        if !active_repairs.insert(tool.name.clone()) {
            return Ok(None);
        }

        Ok(Some(ToolRepairClaim {
            active_repairs: &self.active_tool_repairs,
            tool_name: tool.name.clone(),
        }))
    }
}

/// RAII guard that releases the in-process repair claim on drop.
///
/// Constructed by [`RepairClaims::claim_tool`] when a slot is
/// successfully claimed. Dropping this value removes the tool name from
/// the active-repairs set, making it available for subsequent callers.
pub(super) struct ToolRepairClaim<'a> {
    active_repairs: &'a Mutex<HashSet<String>>,
    tool_name: String,
}

impl Drop for ToolRepairClaim<'_> {
    fn drop(&mut self) {
        match self.active_repairs.lock() {
            Ok(mut active_repairs) => {
                active_repairs.remove(&self.tool_name);
                tracing::debug!(
                    tool_name = %self.tool_name,
                    "repair claim released"
                );
            }
            Err(e) => {
                tracing::error!(
                    tool_name = %self.tool_name,
                    error = %e,
                    "repair-claims mutex is poisoned; cannot release claim"
                );
            }
        }
    }
}
