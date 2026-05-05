//! Wrapper exposing [`BuildSoftwareTool`] as a [`NativeTool`].
//!
//! This module bridges the builder pipeline to the agent tool interface.
//! [`BuildSoftwareTool::execute`] accepts a JSON parameter object,
//! delegates analysis and compilation to a [`SoftwareBuilder`], and
//! returns a structured JSON [`ToolOutput`].  Optional `type` and
//! `language` parameters are resolved through private helpers that
//! centralise fallback and error-message logic.

use super::*;

/// Tool that allows the agent to build software on demand.
pub struct BuildSoftwareTool {
    builder: Arc<dyn SoftwareBuilder>,
}

impl BuildSoftwareTool {
    /// Wraps a [`SoftwareBuilder`] for use as a [`NativeTool`].
    pub fn new(builder: Arc<dyn SoftwareBuilder>) -> Self {
        Self { builder }
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

        let start = std::time::Instant::now();

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

        Ok(ToolOutput::success(output, start.elapsed()))
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
mod tests {
    use super::*;

    use insta::assert_snapshot;

    // resolve_override -------------------------------------------------------

    #[test]
    fn resolve_override_none_returns_fallback() {
        let result: Result<u32, ToolError> =
            BuildSoftwareTool::resolve_override(None, 42u32, "thing", |_| None);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn resolve_override_some_valid_returns_parsed() {
        let result: Result<u32, ToolError> =
            BuildSoftwareTool::resolve_override(Some("one"), 0u32, "thing", |s| {
                (s == "one").then_some(1u32)
            });
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn resolve_override_some_invalid_returns_invalid_parameters() {
        let result: Result<u32, ToolError> =
            BuildSoftwareTool::resolve_override(Some("nope"), 0u32, "thing", |_| None);
        match result.unwrap_err() {
            ToolError::InvalidParameters(msg) => {
                assert_eq!(msg, "unknown thing: nope");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    // resolve_software_type --------------------------------------------------

    #[test]
    fn resolve_software_type_none_preserves_fallback() {
        let result = BuildSoftwareTool::resolve_software_type(None, SoftwareType::Library);
        assert_eq!(result.unwrap(), SoftwareType::Library);
    }

    #[test]
    fn resolve_software_type_all_valid_values() {
        let cases = [
            ("wasm_tool", SoftwareType::WasmTool),
            ("cli_binary", SoftwareType::CliBinary),
            ("library", SoftwareType::Library),
            ("script", SoftwareType::Script),
        ];
        for (input, expected) in cases {
            let result =
                BuildSoftwareTool::resolve_software_type(Some(input), SoftwareType::WasmTool);
            assert_eq!(result.unwrap(), expected, "input: {input}");
        }
    }

    #[test]
    fn resolve_software_type_unknown_value_errors() {
        let result =
            BuildSoftwareTool::resolve_software_type(Some("web_service"), SoftwareType::WasmTool);
        match result.unwrap_err() {
            ToolError::InvalidParameters(msg) => {
                assert_eq!(msg, "unknown type: web_service");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn resolve_software_type_is_case_sensitive() {
        let result =
            BuildSoftwareTool::resolve_software_type(Some("WasmTool"), SoftwareType::Script);
        assert!(result.is_err());
    }

    // resolve_language -------------------------------------------------------

    #[test]
    fn resolve_language_none_preserves_fallback() {
        let result = BuildSoftwareTool::resolve_language(None, Language::Python);
        assert_eq!(result.unwrap(), Language::Python);
    }

    #[test]
    fn resolve_language_all_valid_values() {
        let cases = [
            ("rust", Language::Rust),
            ("python", Language::Python),
            ("typescript", Language::TypeScript),
            ("bash", Language::Bash),
        ];
        for (input, expected) in cases {
            let result = BuildSoftwareTool::resolve_language(Some(input), Language::Rust);
            assert_eq!(result.unwrap(), expected, "input: {input}");
        }
    }

    #[test]
    fn resolve_language_unknown_value_errors() {
        let result = BuildSoftwareTool::resolve_language(Some("go"), Language::Rust);
        match result.unwrap_err() {
            ToolError::InvalidParameters(msg) => {
                assert_eq!(msg, "unknown language: go");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn resolve_language_is_case_sensitive() {
        let result = BuildSoftwareTool::resolve_language(Some("Rust"), Language::Bash);
        assert!(result.is_err());
    }

    #[test]
    fn snapshot_resolve_software_type_unknown_error_message() {
        let err =
            BuildSoftwareTool::resolve_software_type(Some("unknown_value"), SoftwareType::WasmTool)
                .unwrap_err()
                .to_string();
        assert_snapshot!(err);
    }

    #[test]
    fn snapshot_resolve_language_unknown_error_message() {
        let err = BuildSoftwareTool::resolve_language(Some("cobol"), Language::Rust)
            .unwrap_err()
            .to_string();
        assert_snapshot!(err);
    }
}
