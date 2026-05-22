//! Wrapper exposing [`BuildSoftwareTool`] as a [`NativeTool`].
//!
//! This module bridges the builder pipeline to the agent tool interface.
//! [`BuildSoftwareTool::execute`] accepts a JSON parameter object,
//! delegates analysis and compilation to a [`SoftwareBuilder`], and
//! returns a structured JSON [`ToolOutput`].  Optional `type` and
//! `language` parameters are resolved through private helpers that
//! centralize fallback and error-message logic.

use super::*;
use mockable::{Clock, DefaultClock};

/// Tool that allows the agent to build software on demand.
pub struct BuildSoftwareTool {
    builder: Arc<dyn SoftwareBuilder>,
    clock: Arc<dyn Clock>,
}

impl BuildSoftwareTool {
    /// Wraps a [`SoftwareBuilder`] for use as a [`NativeTool`].
    pub fn new(builder: Arc<dyn SoftwareBuilder>) -> Self {
        Self {
            builder,
            clock: Arc::new(DefaultClock),
        }
    }

    /// Resolves an optional string override against a parse closure.
    ///
    /// Returns `fallback` when `override_str` is `None`.  When
    /// `override_str` is `Some(s)`, calls `parse(s)`; if the closure
    /// returns `None` the method produces
    /// [`ToolError::InvalidParameters`] with the message
    /// `"unknown {label}: {s}"`.
    fn resolve_override<T>(
        override_str: Option<&str>,
        fallback: T,
        label: &str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<T, ToolError> {
        match override_str {
            None => Ok(fallback),
            Some(s) => parse(s)
                .ok_or_else(|| ToolError::InvalidParameters(format!("unknown {label}: {s}"))),
        }
    }

    /// Parses an optional `type` parameter override into a
    /// [`SoftwareType`] variant, falling back to `fallback` when absent.
    ///
    /// Accepted values: `"wasm_tool"`, `"cli_binary"`, `"library"`,
    /// `"script"`.  Any other value yields
    /// [`ToolError::InvalidParameters`].
    fn resolve_software_type(
        override_str: Option<&str>,
        fallback: SoftwareType,
    ) -> Result<SoftwareType, ToolError> {
        Self::resolve_override(override_str, fallback, "type", |s| match s {
            "wasm_tool" => Some(SoftwareType::WasmTool),
            "cli_binary" => Some(SoftwareType::CliBinary),
            "library" => Some(SoftwareType::Library),
            "script" => Some(SoftwareType::Script),
            _ => None,
        })
    }

    /// Parses an optional `language` parameter override into a
    /// [`Language`] variant, falling back to `fallback` when absent.
    ///
    /// Accepted values: `"rust"`, `"python"`, `"typescript"`, `"bash"`.
    /// Any other value yields [`ToolError::InvalidParameters`].
    fn resolve_language(
        override_str: Option<&str>,
        fallback: Language,
    ) -> Result<Language, ToolError> {
        Self::resolve_override(override_str, fallback, "language", |s| match s {
            "rust" => Some(Language::Rust),
            "python" => Some(Language::Python),
            "typescript" => Some(Language::TypeScript),
            "bash" => Some(Language::Bash),
            _ => None,
        })
    }
}

impl NativeTool for BuildSoftwareTool {
    /// Stable tool identifier exposed to the model (`build_software`).
    fn name(&self) -> &str {
        "build_software"
    }

    /// Short guidance for agents on when and how to call this tool.
    fn description(&self) -> &str {
        "Build software from a description. IMPORTANT: For tools the agent will use, \
         ALWAYS build Rust WASM tools (type: wasm_tool, language: rust). Only use cli_binary, \
         script, or other types for software meant for human users. The builder scaffolds, \
         implements, compiles, and tests iteratively."
    }

    /// JSON Schema for tool parameters: required `description`, optional `type`,
    /// and optional `language`.
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "Natural language description of what to build"
                },
                "type": {
                    "type": "string",
                    "enum": ["wasm_tool", "cli_binary", "library", "script"],
                    "description": "Type of software to build (optional, will be inferred)"
                },
                "language": {
                    "type": "string",
                    "enum": ["rust", "python", "typescript", "bash"],
                    "description": "Programming language to use (optional, will be inferred)"
                }
            },
            "required": ["description"]
        })
    }

    /// Analyses the requirement, applies optional overrides, runs the build, and
    /// returns a JSON [`ToolOutput`].
    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let description = params
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParameters("missing 'description'".into()))?;

        let start = self.clock.utc();

        // Analyze the requirement
        let mut requirement = self
            .builder
            .analyze(description)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Analysis failed: {}", e)))?;

        // Override type/language if specified
        requirement.software_type = Self::resolve_software_type(
            params.get("type").and_then(|v| v.as_str()),
            requirement.software_type,
        )?;

        requirement.language = Self::resolve_language(
            params.get("language").and_then(|v| v.as_str()),
            requirement.language,
        )?;

        // Build
        let result = self
            .builder
            .build(&requirement)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Build failed: {}", e)))?;

        let output = serde_json::json!({
            "build_id": result.build_id.to_string(),
            "name": result.requirement.name.to_string(),
            "success": result.success,
            "artifact_path": result.artifact_path.display().to_string(),
            "iterations": result.iterations,
            "error": result.error,
            "phases": result.logs.iter().map(|l| format!("{:?}: {}", l.phase, l.message)).collect::<Vec<_>>()
        });

        Ok(ToolOutput::success(
            output,
            self.clock
                .utc()
                .signed_duration_since(start)
                .to_std()
                .unwrap_or_default(),
        ))
    }

    /// Approval is required unless the surrounding job auto-approves tools.
    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    /// Hosted runs are tied to the approval-gated eligibility tier.
    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }
}

#[cfg(test)]
#[path = "wrapper_tests.rs"]
mod tests;
