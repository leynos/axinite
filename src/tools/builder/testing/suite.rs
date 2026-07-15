//! Test-suite data model for built-tool testing.
//!
//! Defines test cases, expected fields, results, suites, and basic
//! auto-generated test cases used by the harness in the parent module.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::tools::wasm::WasmError;

/// Errors during testing.
#[derive(Debug, Error)]
pub enum TestError {
    #[error("Failed to load WASM module: {0}")]
    LoadError(#[from] WasmError),

    #[error("Test execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Test timed out after {0:?}")]
    Timeout(Duration),

    #[error("Output mismatch: expected {expected}, got {actual}")]
    OutputMismatch { expected: String, actual: String },

    #[error("Test assertion failed: {0}")]
    AssertionFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// A single test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    /// Name of the test.
    pub name: String,
    /// Description of what this test verifies.
    pub description: Option<String>,
    /// Input JSON to pass to the tool.
    pub input: serde_json::Value,
    /// Expected output (if exact match required).
    pub expected_output: Option<serde_json::Value>,
    /// Expected fields in output (partial match).
    pub expected_fields: Option<Vec<ExpectedField>>,
    /// Whether the tool should return an error.
    pub expect_error: bool,
    /// Expected error message substring (if expect_error is true).
    pub error_contains: Option<String>,
    /// Timeout for this specific test.
    pub timeout_ms: Option<u64>,
}

impl TestCase {
    /// Create a test case with the given name and input, no expectations,
    /// and the suite's default timeout.
    pub fn new(name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            input,
            expected_output: None,
            expected_fields: None,
            expect_error: false,
            error_contains: None,
            timeout_ms: None,
        }
    }
}

/// An expected field in the output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedField {
    /// JSON path to the field (e.g., "result.value" or "data[0].name").
    pub path: String,
    /// Expected value at that path.
    pub value: Option<serde_json::Value>,
    /// Just check that the field exists (if value is None).
    pub exists: bool,
}

/// Result of running a single test.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Name of the test.
    pub name: String,
    /// Whether the test passed.
    pub passed: bool,
    /// Duration of the test.
    pub duration: Duration,
    /// Error message if failed.
    pub error: Option<String>,
    /// Actual output from the tool.
    pub actual_output: Option<serde_json::Value>,
}

/// A suite of tests for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuite {
    /// Name of the test suite.
    pub name: String,
    /// Description of the suite.
    pub description: Option<String>,
    /// Test cases in the suite.
    pub tests: Vec<TestCase>,
    /// Default timeout for tests in milliseconds.
    pub default_timeout_ms: u64,
}

impl Default for TestSuite {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            description: None,
            tests: Vec::new(),
            default_timeout_ms: 5000,
        }
    }
}

impl TestSuite {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Add a test case.
    pub fn add_test(&mut self, test: TestCase) -> &mut Self {
        self.tests.push(test);
        self
    }

    /// Add a simple input/output test.
    pub fn add_io_test(
        &mut self,
        name: impl Into<String>,
        input: serde_json::Value,
        expected: serde_json::Value,
    ) -> &mut Self {
        let mut test = TestCase::new(name, input);
        test.expected_output = Some(expected);
        self.add_test(test)
    }

    /// Add a test that expects an error.
    pub fn add_error_test(
        &mut self,
        name: impl Into<String>,
        input: serde_json::Value,
        error_contains: impl Into<String>,
    ) -> &mut Self {
        let mut test = TestCase::new(name, input);
        test.expect_error = true;
        test.error_contains = Some(error_contains.into());
        self.add_test(test)
    }
}

/// Generate basic test cases for a tool based on its schema.
#[allow(dead_code)] // Public API for auto-generating test cases
pub fn generate_basic_tests(name: &str, input_schema: &serde_json::Value) -> TestSuite {
    let mut suite = TestSuite::new(format!("{}_basic_tests", name));
    suite.description = Some("Auto-generated basic tests".to_string());

    // Test with empty input
    suite.add_error_test("empty_input", serde_json::json!({}), "");

    add_null_required_fields_test(&mut suite, input_schema);
    add_minimal_valid_input_test(&mut suite, input_schema);

    suite
}

/// Add a test that sets every required field to `null`, expecting an error.
fn add_null_required_fields_test(suite: &mut TestSuite, input_schema: &serde_json::Value) {
    let Some(required) = input_schema.get("required").and_then(|r| r.as_array()) else {
        return;
    };

    let mut null_input = serde_json::Map::new();
    for field_name in required.iter().filter_map(|req| req.as_str()) {
        null_input.insert(field_name.to_string(), serde_json::Value::Null);
    }
    suite.add_error_test(
        "null_required_fields",
        serde_json::Value::Object(null_input),
        "",
    );
}

/// Add a test with placeholder values for every typed schema property.
fn add_minimal_valid_input_test(suite: &mut TestSuite, input_schema: &serde_json::Value) {
    let Some(properties) = input_schema.get("properties").and_then(|p| p.as_object()) else {
        return;
    };

    let mut minimal_input = serde_json::Map::new();
    for (name, prop) in properties {
        let prop_type = prop.get("type").and_then(|t| t.as_str());
        if let Some(value) = prop_type.and_then(placeholder_value) {
            minimal_input.insert(name.clone(), value);
        }
    }

    let mut test = TestCase::new(
        "minimal_valid_input",
        serde_json::Value::Object(minimal_input),
    );
    test.description = Some("Test with minimal valid input".to_string());
    suite.add_test(test);
}

/// Produce a minimal placeholder JSON value for a schema `type` string.
fn placeholder_value(prop_type: &str) -> Option<serde_json::Value> {
    match prop_type {
        "string" => Some(serde_json::Value::String("test".to_string())),
        "integer" | "number" => Some(serde_json::Value::Number(0.into())),
        "boolean" => Some(serde_json::Value::Bool(false)),
        "array" => Some(serde_json::Value::Array(vec![])),
        "object" => Some(serde_json::Value::Object(serde_json::Map::new())),
        _ => None,
    }
}
