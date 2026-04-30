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
        let mut active_repairs =
            self.active_tool_repairs
                .lock()
                .map_err(|error| RepairError::Failed {
                    target_type: "tool".to_string(),
                    target_id: Uuid::nil(),
                    reason: format!("failed to claim repair for {}: {}", tool.name, error),
                })?;

        if !active_repairs.insert(tool.name.clone()) {
            return Ok(None);
        }

        Ok(Some(ToolRepairClaim {
            active_repairs: &self.active_tool_repairs,
            tool_name: tool.name.clone(),
        }))
    }
}

pub(super) struct ToolRepairClaim<'a> {
    active_repairs: &'a Mutex<HashSet<String>>,
    tool_name: String,
}

impl Drop for ToolRepairClaim<'_> {
    fn drop(&mut self) {
        if let Ok(mut active_repairs) = self.active_repairs.lock() {
            active_repairs.remove(&self.tool_name);
        }
    }
}
