//! Echo tool for testing.

use crate::context::JobContext;
use crate::tools::tool::{NativeTool, ToolError, ToolOutput, require_str};

/// Simple echo tool for testing.
pub struct EchoTool;

impl NativeTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echoes back the input message. Useful for testing tool execution."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to echo back"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let message = require_str(&params, "message")?;

        Ok(ToolOutput::text(message, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false // Internal tool, no external data
    }
}
