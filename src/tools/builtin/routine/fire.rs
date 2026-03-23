use super::*;

pub struct RoutineFireTool {
    store: Arc<dyn Database>,
    engine: Arc<RoutineEngine>,
}

impl RoutineFireTool {
    pub fn new(store: Arc<dyn Database>, engine: Arc<RoutineEngine>) -> Self {
        Self { store, engine }
    }

    fn approval_requirement() -> ApprovalRequirement {
        ApprovalRequirement::Always
    }
}

impl NativeTool for RoutineFireTool {
    fn name(&self) -> &str {
        "routine_fire"
    }

    fn description(&self) -> &str {
        "Manually trigger a routine to run immediately, bypassing schedule, trigger type, and cooldown."
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        Self::approval_requirement()
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the routine to fire"
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

        let run_id = self
            .engine
            .fire_manual(routine.id, None)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to fire routine '{}': {e}", name))
            })?;

        let result = serde_json::json!({
            "name": name,
            "run_id": run_id.to_string(),
            "status": "fired",
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_fire_always_requires_approval() {
        assert_eq!(
            RoutineFireTool::approval_requirement(),
            ApprovalRequirement::Always
        );
    }
}
