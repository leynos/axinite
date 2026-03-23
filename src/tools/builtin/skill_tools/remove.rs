//! Skill removal tool implementation.

use std::sync::Arc;

use crate::context::JobContext;
use crate::skills::registry::SkillRegistry;
use crate::tools::builtin::skill_tools::object_schema;
use crate::tools::tool::{
    ApprovalRequirement, HostedToolEligibility, NativeTool, ToolError, ToolOutput, require_str,
};

/// Tool for permanently deleting an installed skill from disk and the registry.
pub struct SkillRemoveTool {
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
}

impl SkillRemoveTool {
    /// Create a new removal tool backed by the shared skill registry.
    pub fn new(registry: Arc<std::sync::RwLock<SkillRegistry>>) -> Self {
        Self { registry }
    }
}

impl NativeTool for SkillRemoveTool {
    fn name(&self) -> &str {
        "skill_remove"
    }

    fn description(&self) -> &str {
        concat!(
            "Permanently remove an installed skill from disk. ",
            "This action cannot be undone — the skill files will be deleted."
        )
    }

    fn parameters_schema(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "name": {
                    "type": "string",
                    "description": "Name of the skill to remove"
                }
            }),
            &["name"],
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let name = require_str(&params, "name")?;

        let skill_path = {
            let guard = self
                .registry
                .read()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
            guard
                .validate_remove(name)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
        };

        crate::skills::registry::SkillRegistry::delete_skill_files(&skill_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        {
            let mut guard = self
                .registry
                .write()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
            guard
                .commit_remove(name)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        }

        let output = serde_json::json!({
            "name": name,
            "status": "removed",
            "message": format!("Skill '{}' has been removed.", name)});

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Always
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }
}
