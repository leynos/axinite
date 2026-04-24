//! Skill installation tool implementation.

use std::sync::Arc;

use crate::context::JobContext;
use crate::skills::catalog::SkillCatalog;
use crate::skills::install_source::{non_blank_raw, trimmed_non_empty};
use crate::skills::registry::{PreparedSkillInstall, SkillInstallPayload, SkillRegistry};
use crate::tools::builtin::skill_fetch::fetch_skill_bytes;
use crate::tools::tool::{
    ApprovalRequirement, HostedToolEligibility, NativeTool, ToolError, ToolOutput,
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

    fn select_install_source<'a>(
        params: &'a serde_json::Value,
    ) -> Result<(&'a str, &'static str), ToolError> {
        let content = non_blank_raw(params.get("content").and_then(|value| value.as_str()));
        let url = trimmed_non_empty(params.get("url").and_then(|value| value.as_str()));
        let name = trimmed_non_empty(params.get("name").and_then(|value| value.as_str()));

        let mut chosen: Option<(&'a str, &'static str)> = None;
        for (value, kind) in [(content, "content"), (url, "url"), (name, "name")] {
            if let Some(value) = value {
                if chosen.is_some() {
                    return Err(Self::exactly_one_source_error());
                }
                chosen = Some((value, kind));
            }
        }

        chosen.ok_or_else(Self::exactly_one_source_error)
    }

    fn exactly_one_source_error() -> ToolError {
        ToolError::InvalidParameters(
            "provide exactly one of 'content', 'url', or 'name'".to_string(),
        )
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
                    "description": "Direct HTTPS URL to a SKILL.md file or .skill archive"
                },
                "content": {
                    "type": "string",
                    "description": "Raw SKILL.md content to install directly"
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let (value, kind) = Self::select_install_source(&params)?;

        let payload = match kind {
            "content" => SkillInstallPayload::Markdown(value.to_string()),
            "url" => SkillInstallPayload::DownloadedBytes(
                fetch_skill_bytes(value)
                    .await
                    .map_err(|error| ToolError::ExecutionFailed(error.to_string()))?,
            ),
            "name" => {
                let slug = self.resolve_catalog_slug(value).await;
                let download_url =
                    crate::skills::catalog::skill_download_url(self.catalog.registry_url(), &slug);
                SkillInstallPayload::DownloadedBytes(
                    fetch_skill_bytes(&download_url)
                        .await
                        .map_err(|error| ToolError::ExecutionFailed(error.to_string()))?,
                )
            }
            _ => unreachable!("source kind is constrained by select_install_source"),
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
        let installed_name = prepared.name().to_string();

        let commit_result = {
            let mut guard = self
                .registry
                .write()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
            guard.commit_install(prepared)
        };

        if let Err(commit_failure) = commit_result {
            let (error, prepared) = commit_failure.into_parts();
            if let Err(cleanup_error) = cleanup_prepared_install(&prepared).await {
                tracing::warn!(
                    "failed to cleanup prepared skill install '{}': {}",
                    prepared.name(),
                    cleanup_error
                );
            }
            return Err(ToolError::ExecutionFailed(error.to_string()));
        }

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
