use super::*;

/// Tool that allows the agent to build software on demand.
pub struct BuildSoftwareTool {
    builder: Arc<dyn SoftwareBuilder>,
}

impl BuildSoftwareTool {
    pub fn new(builder: Arc<dyn SoftwareBuilder>) -> Self {
        Self { builder }
    }
}

#[async_trait]
impl Tool for BuildSoftwareTool {
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
        if let Some(type_str) = params.get("type").and_then(|v| v.as_str()) {
            requirement.software_type = match type_str {
                "wasm_tool" => SoftwareType::WasmTool,
                "cli_binary" => SoftwareType::CliBinary,
                "library" => SoftwareType::Library,
                "script" => SoftwareType::Script,
                other => {
                    return Err(ToolError::InvalidParameters(format!(
                        "unknown type: {other}"
                    )));
                }
            };
        }

        if let Some(lang_str) = params.get("language").and_then(|v| v.as_str()) {
            requirement.language = match lang_str {
                "rust" => Language::Rust,
                "python" => Language::Python,
                "typescript" => Language::TypeScript,
                "bash" => Language::Bash,
                other => {
                    return Err(ToolError::InvalidParameters(format!(
                        "unknown language: {other}"
                    )));
                }
            };
        }

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
