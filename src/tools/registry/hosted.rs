//! Canonical hosted-catalogue projection helpers for `ToolRegistry`.

use std::sync::Arc;

use crate::llm::ToolDefinition;
use crate::tools::tool::{HostedToolCatalogSource, HostedToolEligibility, Tool};

use super::ToolRegistry;

/// Failure modes when selecting a hosted-visible tool from the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedToolLookupError {
    /// No tool with the requested name exists in the registry.
    NotFound,
    /// The tool exists but is not part of the hosted-visible source set.
    Ineligible,
    /// The tool exists in the requested source set but is approval-gated.
    ApprovalGated,
}

impl ToolRegistry {
    /// Return hosted-visible tool definitions for the requested source families.
    pub async fn hosted_tool_definitions(
        &self,
        allowed_sources: &[HostedToolCatalogSource],
    ) -> Vec<ToolDefinition> {
        let mut defs = self
            .tools
            .read()
            .await
            .values()
            .filter_map(|tool| match hosted_tool_lookup(tool, allowed_sources) {
                Ok(()) => Some(ToolDefinition {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.parameters_schema(),
                }),
                Err(_) => None,
            })
            .collect::<Vec<_>>();
        defs.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        defs
    }

    /// Return a hosted-visible tool by name or the reason it is unavailable.
    pub async fn get_hosted_tool(
        &self,
        name: &str,
        allowed_sources: &[HostedToolCatalogSource],
    ) -> Result<Arc<dyn Tool>, HostedToolLookupError> {
        let tool = self
            .get(name)
            .await
            .ok_or(HostedToolLookupError::NotFound)?;
        hosted_tool_lookup(&tool, allowed_sources)?;
        Ok(tool)
    }
}

fn hosted_tool_lookup(
    tool: &Arc<dyn Tool>,
    allowed_sources: &[HostedToolCatalogSource],
) -> Result<(), HostedToolLookupError> {
    if !is_hosted_tool_source_allowed(tool, allowed_sources)
        || tool.domain() != crate::tools::ToolDomain::Orchestrator
        || ToolRegistry::is_protected_tool_name(tool.name())
    {
        return Err(HostedToolLookupError::Ineligible);
    }

    match tool.hosted_tool_eligibility() {
        HostedToolEligibility::Eligible => Ok(()),
        HostedToolEligibility::ApprovalGated => Err(HostedToolLookupError::ApprovalGated),
    }
}

fn is_hosted_tool_source_allowed(
    tool: &Arc<dyn Tool>,
    allowed_sources: &[HostedToolCatalogSource],
) -> bool {
    let Some(tool_source) = tool.hosted_tool_catalog_source() else {
        return false;
    };
    allowed_sources.contains(&tool_source)
}
