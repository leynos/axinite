use super::*;

/// Tool that allows the agent to build software on demand.
pub struct BuildSoftwareTool {
    builder: Arc<dyn SoftwareBuilder>,
}

impl BuildSoftwareTool {
    pub fn new(builder: Arc<dyn SoftwareBuilder>) -> Self {
        Self { builder }
    }

    fn resolve_software_type(
        override_str: Option<&str>,
        fallback: SoftwareType,
    ) -> Result<SoftwareType, ToolError> {
        match override_str {
            None => Ok(fallback),
            Some("wasm_tool") => Ok(SoftwareType::WasmTool),
            Some("cli_binary") => Ok(SoftwareType::CliBinary),
            Some("library") => Ok(SoftwareType::Library),
            Some("script") => Ok(SoftwareType::Script),
            Some(other) => Err(ToolError::InvalidParameters(format!(
                "unknown type: {other}"
            ))),
        }
    }

    fn resolve_language(
        override_str: Option<&str>,
        fallback: Language,
    ) -> Result<Language, ToolError> {
        match override_str {
            None => Ok(fallback),
            Some("rust") => Ok(Language::Rust),
            Some("python") => Ok(Language::Python),
            Some("typescript") => Ok(Language::TypeScript),
            Some("bash") => Ok(Language::Bash),
            Some(other) => Err(ToolError::InvalidParameters(format!(
                "unknown language: {other}"
            ))),
        }
    }
}

impl NativeTool for BuildSoftwareTool {
    fn name(&self) -> &str {
        "build_software"
    }

    fn description(&self) -> &str {
        "Build software from a description. IMPORTANT: For tools the agent will use, \
         ALWAYS build Rust WASM tools (type: wasm_tool, language: rust). Only use cli_binary, \
         script, or other types for software meant for human users. The builder scaffolds, \
         implements, compiles, and tests iteratively."
    }

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

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }
}
