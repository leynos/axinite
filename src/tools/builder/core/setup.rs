//! Builder setup and prompt construction helpers.
//!
//! This module defines [`LlmSoftwareBuilder`] and the setup logic that wires a
//! [`BuilderConfig`], an [`LlmProvider`], and a [`ToolRegistry`] into the
//! builder runtime. It also owns the builder-specific system prompt and WASM
//! scaffolding guidance.

use super::*;

/// LLM-powered software builder.
pub struct LlmSoftwareBuilder {
    pub(super) config: BuilderConfig,
    pub(super) llm: Arc<dyn LlmProvider>,
    pub(super) tools: Arc<ToolRegistry>,
}

impl LlmSoftwareBuilder {
    /// Create a new LLM-based software builder.
    pub fn new(
        config: BuilderConfig,
        llm: Arc<dyn LlmProvider>,
        tools: Arc<ToolRegistry>,
    ) -> Result<Self, std::io::Error> {
        std::fs::create_dir_all(&config.build_dir)?;
        Ok(Self { config, llm, tools })
    }

    /// Get the build tools available for the build loop.
    pub(super) async fn get_build_tools(&self) -> Vec<ToolDefinition> {
        // Only include tools useful for building software
        self.tools
            .tool_definitions_for(&[
                "shell",
                "read_file",
                "write_file",
                "list_dir",
                "apply_patch",
                "http", // For fetching docs/deps
            ])
            .await
    }

    /// Create the system prompt for the build agent.
    pub(super) fn build_system_prompt(&self, requirement: &BuildRequirement) -> String {
        let mut prompt = format!(
            r#"You are a software developer building a program.

## Task
Build: {name}
Description: {description}
Type: {software_type:?}
Language: {language:?}

## Process
1. Create the project structure with necessary files
2. Implement the code based on the requirements
3. Build/compile if needed
4. Run tests to verify correctness
5. Fix any errors and iterate

## Guidelines
- Write clean, well-structured code
- Handle errors appropriately
- Add minimal but useful comments
- Follow idiomatic patterns for the language
- Test edge cases

## Tools Available
- shell: Run build commands, tests, install dependencies
- read_file: Read existing files
- write_file: Create new files
- apply_patch: Edit existing files surgically
- list_dir: Explore project structure
"#,
            name = requirement.name,
            description = requirement.description,
            software_type = requirement.software_type,
            language = requirement.language,
        );

        // Add tool-specific context when building WASM tools
        if requirement.software_type == SoftwareType::WasmTool {
            prompt.push_str(&self.wasm_tool_context());
        }

        prompt
    }

    /// Get additional context for building WASM tools.
    fn wasm_tool_context(&self) -> String {
        r#"

## WASM Tool Requirements

You are building a WASM Component tool for an autonomous agent using the WASM Component Model.
The tool MUST use `wit_bindgen` and `cargo-component` to build.

## Available Host Functions (from WIT interface)

The host provides these functions via `near::agent::host`:

```rust
// Logging (always available)
host::log(level: LogLevel, message: &str);  // LogLevel: Trace, Debug, Info, Warn, Error

// Time (always available)
host::now_millis() -> u64;  // Unix timestamp in milliseconds

// Workspace (if capability granted)
host::workspace_read(path: &str) -> Option<String>;

// HTTP (if capability granted)
host::http_request(method: &str, url: &str, headers_json: &str, body: Option<Vec<u8>>)
    -> Result<HttpResponse, String>;
// HttpResponse has: status: u16, headers_json: String, body: Vec<u8>

// Tool invocation (if capability granted)
host::tool_invoke(alias: &str, params_json: &str) -> Result<String, String>;

// Secrets (if capability granted) - can only CHECK existence, not read values
host::secret_exists(name: &str) -> bool;
```

## Project Structure

```
my_tool/
├── Cargo.toml
├── wit/
│   └── tool.wit      # Copy from agent's wit/tool.wit
└── src/
    └── lib.rs
```

## Cargo.toml Template

```toml
[package]
name = "my_tool"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = "0.54.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

## src/lib.rs Template

```rust
// Generate bindings from the WIT interface
wit_bindgen::generate!({
    world: "sandboxed-tool",
    path: "wit/tool.wit",
});

use serde::{Deserialize, Serialize};
use exports::near::agent::tool::{Guest, Request, Response};
use near::agent::host::{self, LogLevel};

// Your input/output types
#[derive(Deserialize)]
struct MyInput {
    // Define parameters here
}

#[derive(Serialize)]
struct MyOutput {
    // Define output here
}

struct MyTool;

impl Guest for MyTool {
    fn execute(req: Request) -> Response {
        // Parse input
        let input: MyInput = match serde_json::from_str(&req.params) {
            Ok(i) => i,
            Err(e) => return Response {
                output: None,
                error: Some(format!("Invalid input: {}", e)),
            },
        };

        host::log(LogLevel::Info, &format!("Processing request..."));

        // Your implementation here
        let output = MyOutput { /* ... */ };

        // Return success
        Response {
            output: Some(serde_json::to_string(&output).unwrap()),
            error: None,
        }
    }

    fn schema() -> String {
        serde_json::json!({
            "type": "object",
            "properties": {
                // Define your JSON Schema here
            },
            "required": []
        }).to_string()
    }

    fn description() -> String {
        "Description of what this tool does".to_string()
    }
}

export!(MyTool);
```

## Build Commands

```bash
# Install cargo-component (one time)
cargo install cargo-component

# Build the WASM component
cargo component build --release

# Output: target/wasm32-wasip2/release/my_tool.wasm
```

## Capabilities File (my_tool.capabilities.json)

Create alongside the .wasm file to grant capabilities:

```json
{
    "http": {
        "allowed_endpoints": [
            {"host": "api.example.com", "path_prefix": "/v1/"}
        ]
    },
    "workspace": true,
    "secrets": {
        "allowed": ["API_KEY"]
    }
}
```

## Important Notes

1. NEVER panic - always return Response with error field set
2. Secrets are NEVER exposed to WASM - use placeholders like `{API_KEY}` in URLs
   and the host will inject the real value
3. HTTP requests are rate-limited and only allowed to endpoints in capabilities
4. Keep the tool focused on one thing - small, composable tools are better

"#
        .to_string()
    }
}
