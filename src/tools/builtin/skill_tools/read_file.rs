//! Skill bundle file read tool implementation.

use std::sync::Arc;

use crate::context::JobContext;
use crate::skills::LoadedSkill;
use crate::skills::file_read::{SkillReadFileResponse, read_skill_file};
use crate::skills::registry::SkillRegistry;
use crate::tools::tool::{NativeTool, ToolError, ToolOutput, require_str};

use super::strict_object_schema;

/// Tool for reading text files from a loaded skill bundle by bundle-relative path.
pub struct SkillReadFileTool {
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
}

impl SkillReadFileTool {
    /// Create a new read tool backed by the shared skill registry.
    pub fn new(registry: Arc<std::sync::RwLock<SkillRegistry>>) -> Self {
        Self { registry }
    }

    fn lookup_skill(&self, skill_name: &str) -> Result<Option<LoadedSkill>, ToolError> {
        let guard = self
            .registry
            .read()
            .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
        Ok(guard.find_by_name(skill_name).cloned())
    }
}

impl NativeTool for SkillReadFileTool {
    fn name(&self) -> &str {
        "skill_read_file"
    }

    fn description(&self) -> &str {
        "Read a text file from a loaded skill bundle using a bundle-relative path."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        strict_object_schema(
            serde_json::json!({
                "skill": {
                    "type": "string",
                    "description": "Loaded skill name exactly as advertised to the model."
                },
                "path": {
                    "type": "string",
                    "description": "Bundle-relative path, such as SKILL.md or references/usage.md."
                }
            }),
            &["skill", "path"],
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let skill_name = require_str(&params, "skill")?.trim();
        let path = require_str(&params, "path")?;
        if skill_name.is_empty() {
            return Err(ToolError::InvalidParameters(
                "skill must not be empty".to_string(),
            ));
        }

        let response = match self.lookup_skill(skill_name)? {
            Some(skill) => read_skill_file(&skill, path).await,
            None => {
                tracing::debug!(
                    skill_id = %skill_name,
                    path = %path,
                    "skill_read_file: skill not found in registry"
                );
                SkillReadFileResponse::unknown_skill(skill_name, path)
            }
        };

        let elapsed = start.elapsed();
        match &response {
            SkillReadFileResponse::Success(success) => {
                tracing::debug!(
                    skill_id = %skill_name,
                    path = %path,
                    size = success.content.len(),
                    duration_ms = elapsed.as_millis(),
                    "skill_read_file: success"
                );
            }
            SkillReadFileResponse::Error(error) => {
                tracing::debug!(
                    skill_id = %skill_name,
                    path = %path,
                    error_code = ?error.error.code,
                    duration_ms = elapsed.as_millis(),
                    "skill_read_file: error"
                );
            }
        }

        let result = serde_json::to_value(response).map_err(|error| {
            ToolError::ExecutionFailed(format!("failed to serialize skill read response: {error}"))
        })?;
        Ok(ToolOutput::success(result, elapsed))
    }
}
