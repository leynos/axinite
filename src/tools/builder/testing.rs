//! Testing harness for built tools.
//!
//! Provides automated testing of generated tools before registration,
//! ensuring they work correctly with various inputs.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::context::JobContext;
use crate::tools::tool::Tool;
use crate::tools::wasm::{Capabilities, WasmToolRuntime, WasmToolWrapper};

mod suite;

#[cfg(test)]
mod tests;

// Glob re-export keeps every suite item (including `ExpectedField` and
// `generate_basic_tests`, which are only exercised by tests today) reachable
// at its original path without tripping `unused_imports` in non-test builds.
pub use suite::*;

/// Harness for running tests against WASM tools.
pub struct TestHarness {
    runtime: Arc<WasmToolRuntime>,
    capabilities: Capabilities,
    default_timeout: Duration,
}

impl TestHarness {
    /// Create a harness with no capabilities and a 5-second default timeout.
    pub fn new(runtime: Arc<WasmToolRuntime>) -> Self {
        Self {
            runtime,
            capabilities: Capabilities::none(),
            default_timeout: Duration::from_secs(5),
        }
    }

    /// Set capabilities for test execution.
    pub fn with_capabilities(mut self, caps: Capabilities) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set default timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Run a test suite against a WASM file.
    pub async fn run_suite_file(
        &self,
        wasm_path: &Path,
        suite: &TestSuite,
    ) -> Result<Vec<TestResult>, TestError> {
        let bytes = tokio::fs::read(wasm_path).await?;
        self.run_suite_bytes(&bytes, suite).await
    }

    /// Run a test suite against WASM bytes.
    pub async fn run_suite_bytes(
        &self,
        wasm_bytes: &[u8],
        suite: &TestSuite,
    ) -> Result<Vec<TestResult>, TestError> {
        // Prepare the module
        let prepared = self.runtime.prepare(&suite.name, wasm_bytes, None).await?;

        // Create a tool wrapper for execution
        let tool = WasmToolWrapper::new(
            Arc::clone(&self.runtime),
            prepared,
            self.capabilities.clone(),
        );

        let mut results = Vec::with_capacity(suite.tests.len());

        for test in &suite.tests {
            let result = self.run_test(&tool, test, suite.default_timeout_ms).await;
            results.push(result);
        }

        Ok(results)
    }

    /// Run a single test case.
    async fn run_test(
        &self,
        tool: &WasmToolWrapper,
        test: &TestCase,
        default_timeout_ms: u64,
    ) -> TestResult {
        let timeout = Duration::from_millis(test.timeout_ms.unwrap_or(default_timeout_ms));
        let start = Instant::now();
        let ctx = JobContext::default();

        // Execute with timeout
        let exec_result = tokio::time::timeout(timeout, async {
            tool.execute(test.input.clone(), &ctx).await
        })
        .await;

        let duration = start.elapsed();

        let (failure, actual_output) = match exec_result {
            Err(_) => (Some(format!("Test timed out after {:?}", timeout)), None),
            Ok(Err(e)) => (judge_execution_error(test, &e.to_string()), None),
            Ok(Ok(output)) => (judge_output(test, &output.result), Some(output.result)),
        };

        TestResult {
            name: test.name.clone(),
            passed: failure.is_none(),
            duration,
            error: failure,
            actual_output,
        }
    }
}

/// Describe a failed `error_contains` expectation, or `None` if it holds.
fn error_text_mismatch(test: &TestCase, error_str: &str) -> Option<String> {
    let matches = test
        .error_contains
        .as_ref()
        .is_none_or(|expected| error_str.contains(expected));
    (!matches).then(|| {
        format!(
            "Expected error containing '{}', got: {}",
            test.error_contains.as_deref().unwrap_or(""),
            error_str
        )
    })
}

/// Judge a tool execution error against the test's error expectation.
fn judge_execution_error(test: &TestCase, error_str: &str) -> Option<String> {
    if test.expect_error {
        error_text_mismatch(test, error_str)
    } else {
        Some(format!("Unexpected error: {}", error_str))
    }
}

/// Judge a successful tool output; `None` means the test passed.
fn judge_output(test: &TestCase, actual: &serde_json::Value) -> Option<String> {
    // Check if output contains an error field
    if let Some(error_val) = actual.get("error") {
        return if test.expect_error {
            error_text_mismatch(test, error_val.as_str().unwrap_or(""))
        } else {
            Some(format!("Unexpected error in output: {}", error_val))
        };
    }

    // Verify expected output
    if let Some(ref expected) = test.expected_output
        && actual != expected
    {
        return Some(format!(
            "Output mismatch:\nExpected: {}\nActual: {}",
            serde_json::to_string_pretty(expected).unwrap_or_default(),
            serde_json::to_string_pretty(actual).unwrap_or_default()
        ));
    }

    // Verify expected fields
    test.expected_fields.as_deref().and_then(|fields| {
        fields
            .iter()
            .find_map(|field| field_mismatch(field, actual))
    })
}

/// Describe how `actual` fails `field`'s expectation, or `None` if it holds.
fn field_mismatch(field: &ExpectedField, actual: &serde_json::Value) -> Option<String> {
    let field_value = get_json_path(actual, &field.path);

    if field.exists && field_value.is_none() {
        return Some(format!("Missing expected field: {}", field.path));
    }

    match field.value {
        Some(ref expected_value) if field_value != Some(expected_value) => Some(format!(
            "Field '{}' mismatch: expected {:?}, got {:?}",
            field.path, expected_value, field_value
        )),
        _ => None,
    }
}

/// Get a value from a JSON object by path (e.g., "foo.bar[0].baz").
fn get_json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;

    for segment in path.split('.') {
        // Handle array indexing like "items[0]"
        if let Some(bracket_pos) = segment.find('[') {
            let key = &segment[..bracket_pos];
            let index_str = &segment[bracket_pos + 1..segment.len() - 1];

            if !key.is_empty() {
                current = current.get(key)?;
            }

            let index: usize = index_str.parse().ok()?;
            current = current.get(index)?;
        } else {
            current = current.get(segment)?;
        }
    }

    Some(current)
}
