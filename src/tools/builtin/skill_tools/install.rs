//! Skill installation tool implementation.

use std::sync::Arc;

use crate::context::JobContext;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::{PreparedSkillInstall, SkillInstallPayload, SkillRegistry};
use crate::tools::builtin::skill_fetch::fetch_skill_bytes;
use crate::tools::tool::{
    ApprovalRequirement, HostedToolEligibility, NativeTool, ToolError, ToolOutput, require_str,
};

/// Install skills from inline content, a URL, or a catalogue lookup.
pub struct SkillInstallTool {
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
    catalog: Arc<SkillCatalog>,
}

impl SkillInstallTool {
    /// Create a skill installer backed by the shared registry and catalogue.
    pub fn new(
        registry: Arc<std::sync::RwLock<SkillRegistry>>,
        catalog: Arc<SkillCatalog>,
    ) -> Self {
        Self { registry, catalog }
    }

    async fn resolve_catalog_slug(&self, name_or_slug: &str) -> String {
        if name_or_slug.contains('/') {
            return name_or_slug.to_string();
        }

        self.catalog
            .search(name_or_slug)
            .await
            .results
            .into_iter()
            .find(|entry| {
                entry.slug.eq_ignore_ascii_case(name_or_slug)
                    || entry.name.eq_ignore_ascii_case(name_or_slug)
                    || entry
                        .slug
                        .rsplit('/')
                        .next()
                        .is_some_and(|segment| segment.eq_ignore_ascii_case(name_or_slug))
            })
            .map(|entry| entry.slug)
            .unwrap_or_else(|| name_or_slug.to_string())
    }
}

impl NativeTool for SkillInstallTool {
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
                    "description": "Skill slug from the ClawHub catalogue"
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

        let payload = if let Some(raw) = params.get("content").and_then(|value| value.as_str()) {
            SkillInstallPayload::Markdown(raw.to_string())
        } else if let Some(url) = params.get("url").and_then(|value| value.as_str()) {
            SkillInstallPayload::DownloadedBytes(fetch_skill_bytes(url).await?)
        } else {
            let slug = self.resolve_catalog_slug(name).await;
            let download_url =
                crate::skills::catalog::skill_download_url(self.catalog.registry_url(), &slug);
            SkillInstallPayload::DownloadedBytes(fetch_skill_bytes(&download_url).await?)
        };

        let install_root = {
            let guard = self
                .registry
                .read()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
            guard.install_target_dir().to_path_buf()
        };

        let prepared =
            crate::skills::registry::SkillRegistry::prepare_install_to_disk(&install_root, payload)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let commit_result = {
            let mut guard = self
                .registry
                .write()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
            guard.commit_install(&prepared)
        };

        if let Err(error) = commit_result {
            cleanup_prepared_install(&prepared).await?;
            return Err(ToolError::ExecutionFailed(error.to_string()));
        }

        let installed_name = prepared.name().to_string();
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

async fn cleanup_prepared_install(prepared: &PreparedSkillInstall) -> Result<(), ToolError> {
    SkillRegistry::cleanup_prepared_install(prepared)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    Ok(())
}
