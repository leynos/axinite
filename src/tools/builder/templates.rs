//! Code templates for common tool patterns.
//!
//! Templates provide scaffolding that the LLM fills in, reducing the chance
//! of structural errors and ensuring consistent patterns.

use std::collections::HashMap;

mod sources;

use sources::{
    BASH_SCRIPT, CLI_CARGO_TOML, CLI_MAIN_RS, PYTHON_SCRIPT, WASM_CARGO_TOML, WASM_COMPUTE_LIB_RS,
    WASM_HTTP_LIB_RS, WASM_TRANSFORM_LIB_RS,
};

use serde::{Deserialize, Serialize};

/// Type of template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateType {
    /// WASM tool with HTTP capability.
    WasmHttpTool,
    /// WASM tool for data transformation.
    WasmTransformTool,
    /// WASM tool for computation.
    WasmComputeTool,
    /// CLI application.
    CliBinary,
    /// Python script.
    PythonScript,
    /// Bash script.
    BashScript,
}

/// A code template with placeholders.
#[derive(Debug, Clone)]
pub struct Template {
    pub template_type: TemplateType,
    pub name: &'static str,
    pub description: &'static str,
    pub files: Vec<TemplateFile>,
}

/// A file within a template.
#[derive(Debug, Clone)]
pub struct TemplateFile {
    pub path: &'static str,
    pub content: &'static str,
    pub is_required: bool,
}

/// Engine for rendering templates with variable substitution.
#[derive(Debug, Default)]
pub struct TemplateEngine {
    variables: HashMap<String, String>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a template variable.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.variables.insert(key.into(), value.into());
        self
    }

    /// Render a template string, replacing {{variable}} placeholders.
    pub fn render(&self, template: &str) -> String {
        let mut result = template.to_string();
        for (key, value) in &self.variables {
            let placeholder = format!("{{{{{}}}}}", key);
            result = result.replace(&placeholder, value);
        }
        result
    }

    /// Render all files in a template.
    pub fn render_template(&self, template: &Template) -> Vec<(String, String)> {
        template
            .files
            .iter()
            .map(|f| (self.render(f.path), self.render(f.content)))
            .collect()
    }
}

impl Template {
    /// Get template by type.
    pub fn get(template_type: TemplateType) -> Self {
        match template_type {
            TemplateType::WasmHttpTool => Self::wasm_http_tool(),
            TemplateType::WasmTransformTool => Self::wasm_transform_tool(),
            TemplateType::WasmComputeTool => Self::wasm_compute_tool(),
            TemplateType::CliBinary => Self::cli_binary(),
            TemplateType::PythonScript => Self::python_script(),
            TemplateType::BashScript => Self::bash_script(),
        }
    }

    fn wasm_http_tool() -> Self {
        Self {
            template_type: TemplateType::WasmHttpTool,
            name: "WASM HTTP Tool",
            description: "A WASM tool that makes HTTP requests to external APIs",
            files: vec![
                TemplateFile {
                    path: "Cargo.toml",
                    content: WASM_CARGO_TOML,
                    is_required: true,
                },
                TemplateFile {
                    path: "src/lib.rs",
                    content: WASM_HTTP_LIB_RS,
                    is_required: true,
                },
            ],
        }
    }

    fn wasm_transform_tool() -> Self {
        Self {
            template_type: TemplateType::WasmTransformTool,
            name: "WASM Transform Tool",
            description: "A WASM tool that transforms data (JSON, text, etc.)",
            files: vec![
                TemplateFile {
                    path: "Cargo.toml",
                    content: WASM_CARGO_TOML,
                    is_required: true,
                },
                TemplateFile {
                    path: "src/lib.rs",
                    content: WASM_TRANSFORM_LIB_RS,
                    is_required: true,
                },
            ],
        }
    }

    fn wasm_compute_tool() -> Self {
        Self {
            template_type: TemplateType::WasmComputeTool,
            name: "WASM Compute Tool",
            description: "A WASM tool for pure computation (no I/O)",
            files: vec![
                TemplateFile {
                    path: "Cargo.toml",
                    content: WASM_CARGO_TOML,
                    is_required: true,
                },
                TemplateFile {
                    path: "src/lib.rs",
                    content: WASM_COMPUTE_LIB_RS,
                    is_required: true,
                },
            ],
        }
    }

    fn cli_binary() -> Self {
        Self {
            template_type: TemplateType::CliBinary,
            name: "CLI Binary",
            description: "A command-line application with argument parsing",
            files: vec![
                TemplateFile {
                    path: "Cargo.toml",
                    content: CLI_CARGO_TOML,
                    is_required: true,
                },
                TemplateFile {
                    path: "src/main.rs",
                    content: CLI_MAIN_RS,
                    is_required: true,
                },
            ],
        }
    }

    fn python_script() -> Self {
        Self {
            template_type: TemplateType::PythonScript,
            name: "Python Script",
            description: "A Python script with argument parsing",
            files: vec![TemplateFile {
                path: "{{name}}.py",
                content: PYTHON_SCRIPT,
                is_required: true,
            }],
        }
    }

    fn bash_script() -> Self {
        Self {
            template_type: TemplateType::BashScript,
            name: "Bash Script",
            description: "A Bash script with argument handling",
            files: vec![TemplateFile {
                path: "{{name}}.sh",
                content: BASH_SCRIPT,
                is_required: true,
            }],
        }
    }
}

#[cfg(test)]
mod tests;
