//! Skill installation tool implementation.

use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::builtin::skill_fetch::fetch_skill_content;
use crate::tools::tool::{
    ApprovalRequirement, HostedToolEligibility, Tool, ToolError, ToolOutput, require_str,
};

pub struct SkillInstallTool {
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
    catalog: Arc<SkillCatalog>,
}

impl SkillInstallTool {
    pub fn new(
        registry: Arc<std::sync::RwLock<SkillRegistry>>,
        catalog: Arc<SkillCatalog>,
    ) -> Self {
        Self { registry, catalog }
    }
}

#[async_trait]
impl Tool for SkillInstallTool {
    fn name(&self) -> &str {
        "skill_install"
    }

    fn description(&self) -> &str {
        "Install a skill from SKILL.md content, a URL, or by name from the ClawHub catalog."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name or slug (from search results)"
                },
                "url": {
                    "type": "string",
                    "description": "Direct URL to a SKILL.md file"
                },
                "content": {
                    "type": "string",
                    "description": "Raw SKILL.md content to install directly"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let name = require_str(&params, "name")?;

        let content = if let Some(raw) = params.get("content").and_then(|value| value.as_str()) {
            raw.to_string()
        } else if let Some(url) = params.get("url").and_then(|value| value.as_str()) {
            fetch_skill_content(url).await?
        } else {
            let download_url =
                crate::skills::catalog::skill_download_url(self.catalog.registry_url(), name);
            fetch_skill_content(&download_url).await?
        };

        let (user_dir, skill_name_from_parse) = {
            let guard = self
                .registry
                .read()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;

            let normalized = crate::skills::normalize_line_endings(&content);
            let parsed = crate::skills::parser::parse_skill_md(&normalized)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            let skill_name = parsed.manifest.name.clone();

            if guard.has(&skill_name) {
                return Err(ToolError::ExecutionFailed(format!(
                    "Skill '{}' already exists",
                    skill_name
                )));
            }

            (guard.install_target_dir().to_path_buf(), skill_name)
        };

        let (skill_name, loaded_skill) =
            crate::skills::registry::SkillRegistry::prepare_install_to_disk(
                &user_dir,
                &skill_name_from_parse,
                &crate::skills::normalize_line_endings(&content),
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let installed_name = {
            let mut guard = self
                .registry
                .write()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
            guard
                .commit_install(&skill_name, loaded_skill)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            skill_name
        };

        let output = serde_json::json!({
            "name": installed_name,
            "status": "installed",
            "trust": "installed",
            "message": format!(
                "Skill '{}' installed successfully. It will activate when matching keywords are detected.",
                installed_name
            ),
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }
}
