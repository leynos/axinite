//! Builder implementation for requirement analysis, build execution, and repair.
//!
//! This module wires [`LlmSoftwareBuilder`] into the native builder trait.
//! It owns the LLM-backed requirement analysis path, project-directory lifecycle,
//! and timeout-wrapped build execution.

use super::*;

fn extract_first_json_object(response: &str) -> Option<&str> {
    for (start, ch) in response.char_indices() {
        if ch != '{' {
            continue;
        }

        if let Some(object) = extract_json_object_at(response, start) {
            return Some(object);
        }
    }

    None
}

/// Find the JSON object beginning at `start`, returning it when it parses.
fn extract_json_object_at(response: &str, start: usize) -> Option<&str> {
    let mut scanner = JsonObjectScanner::default();

    for (offset, current) in response[start..].char_indices() {
        if scanner.advance(current) == ScanOutcome::Closed {
            let candidate = &response[start..start + offset + current.len_utf8()];
            return parse_json_object(candidate);
        }
    }

    None
}

/// Return `candidate` when it parses as JSON.
fn parse_json_object(candidate: &str) -> Option<&str> {
    if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
        Some(candidate)
    } else {
        None
    }
}

/// Whether a scanned character closed the JSON object being tracked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanOutcome {
    /// The object is still open.
    Continue,
    /// The object's closing brace was reached.
    Closed,
}

/// Tracks brace depth and string state while scanning a JSON object.
#[derive(Default)]
struct JsonObjectScanner {
    depth: usize,
    in_string: bool,
    escaped: bool,
}

impl JsonObjectScanner {
    /// Feed one character, reporting when the enclosing object closes.
    fn advance(&mut self, current: char) -> ScanOutcome {
        if self.in_string {
            self.advance_in_string(current);
            return ScanOutcome::Continue;
        }

        match current {
            '"' => self.in_string = true,
            '{' => self.depth += 1,
            '}' => {
                self.depth = self.depth.saturating_sub(1);
                if self.depth == 0 {
                    return ScanOutcome::Closed;
                }
            }
            _ => {}
        }

        ScanOutcome::Continue
    }

    /// Advance the string-literal state for one character.
    fn advance_in_string(&mut self, current: char) {
        if self.escaped {
            self.escaped = false;
            return;
        }

        match current {
            '\\' => self.escaped = true,
            '"' => self.in_string = false,
            _ => {}
        }
    }
}

impl NativeSoftwareBuilder for LlmSoftwareBuilder {
    async fn analyze(&self, description: &str) -> Result<BuildRequirement, AgentToolError> {
        // Use LLM to parse the description
        let reasoning =
            Reasoning::new(self.llm.clone()).with_model_name(self.llm.active_model_name());

        let prompt = format!(
            r#"Analyse this software requirement and extract structured information.

Description: {}

IMPORTANT: If this is a "tool" that the agent will use (e.g., "calendar tool", "email tool",
"API client tool"), you MUST use:
- software_type: "wasm_tool"
- language: "rust"

Only use cli_binary/script/library for software meant for human end-users, not agent tools.

Respond with a JSON object containing:
- name: A short identifier (snake_case)
- description: What the software should do
- software_type: One of "wasm_tool", "cli_binary", "library", "script", "web_service"
  (PREFER "wasm_tool" for agent-usable tools)
- language: One of "rust", "python", "typescript", "javascript", "go", "bash"
  (PREFER "rust" for wasm_tool)
- input_spec: Expected input format (optional)
- output_spec: Expected output format (optional)
- dependencies: List of external dependencies needed
- capabilities: For WASM tools, list needed capabilities (http, workspace, secrets)

JSON:"#,
            description
        );

        let ctx = ReasoningContext::new().with_message(ChatMessage::user(&prompt));

        let response = reasoning
            .respond(&ctx)
            .await
            .map_err(|e| AgentToolError::BuilderFailed(format!("Analysis failed: {}", e)))?;

        let json_str = extract_first_json_object(&response).unwrap_or(&response);

        serde_json::from_str(json_str).map_err(|e| {
            AgentToolError::BuilderFailed(format!("Failed to parse requirement: {}", e))
        })
    }

    async fn build(&self, requirement: &BuildRequirement) -> Result<BuildResult, AgentToolError> {
        // Create project directory
        let project_dir = self.config.build_dir.join(requirement.name.as_str());
        if project_dir.exists() {
            tokio::fs::remove_dir_all(&project_dir).await.map_err(|e| {
                AgentToolError::BuilderFailed(format!("Failed to clean project dir: {}", e))
            })?;
        }
        tokio::fs::create_dir_all(&project_dir).await.map_err(|e| {
            AgentToolError::BuilderFailed(format!("Failed to create project dir: {}", e))
        })?;

        // Run the build loop with timeout
        let result = tokio::time::timeout(
            self.config.timeout,
            self.execute_build_loop(requirement, &project_dir),
        )
        .await;

        match result {
            Ok(Ok(build_result)) => Ok(build_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AgentToolError::BuilderFailed("Build timed out".into())),
        }
    }

    async fn repair(
        &self,
        result: &BuildResult,
        error: &str,
    ) -> Result<BuildResult, AgentToolError> {
        // Create a new requirement with repair context
        let mut requirement = result.requirement.clone();
        requirement.description = format!(
            "{}\n\nPrevious build failed with error:\n{}\n\nFix the issues and rebuild.",
            requirement.description, error
        );

        // Rebuild (preserving project directory if it exists)
        NativeSoftwareBuilder::build(self, &requirement).await
    }
}

#[cfg(test)]
#[path = "builder_impl_tests.rs"]
mod tests;
