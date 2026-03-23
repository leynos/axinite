//! Built-in tool for permanently deleting routines and their run history.
//!
//! This module owns the destructive routine-deletion path and refreshes the
//! routine-engine event cache after a successful delete.

use super::*;

pub struct RoutineDeleteTool {
    store: Arc<dyn Database>,
    engine: Arc<RoutineEngine>,
}

impl RoutineDeleteTool {
    pub fn new(store: Arc<dyn Database>, engine: Arc<RoutineEngine>) -> Self {
        Self { store, engine }
    }
}

impl NativeTool for RoutineDeleteTool {
    fn name(&self) -> &str {
        "routine_delete"
    }

    fn description(&self) -> &str {
        "Delete a routine permanently. This also removes all run history."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the routine to delete"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        let routine = self
            .store
            .get_routine_by_name(&ctx.user_id, name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("DB error: {e}")))?
            .ok_or_else(|| ToolError::ExecutionFailed(format!("routine '{}' not found", name)))?;

        let deleted = self
            .store
            .delete_routine(routine.id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to delete: {e}")))?;

        // Refresh event cache
        self.engine.refresh_event_cache().await;

        let result = serde_json::json!({
            "name": name,
            "deleted": deleted,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}
