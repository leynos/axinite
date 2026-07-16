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

        match exec_result {
            Err(_) => TestResult {
                name: test.name.clone(),
                passed: false,
                duration,
                error: Some(format!("Test timed out after {:?}", timeout)),
                actual_output: None,
            },
            Ok(Err(e)) => {
                // Execution error
                if test.expect_error {
                    let error_str = e.to_string();
                    let matches = test
                        .error_contains
                        .as_ref()
                        .is_none_or(|expected| error_str.contains(expected));

                    TestResult {
                        name: test.name.clone(),
                        passed: matches,
                        duration,
                        error: if matches {
                            None
                        } else {
                            Some(format!(
                                "Expected error containing '{}', got: {}",
                                test.error_contains.as_deref().unwrap_or(""),
                                error_str
                            ))
                        },
                        actual_output: None,
                    }
                } else {
                    TestResult {
                        name: test.name.clone(),
                        passed: false,
                        duration,
                        error: Some(format!("Unexpected error: {}", e)),
                        actual_output: None,
                    }
                }
            }
            Ok(Ok(output)) => {
                let actual = output.result;

                // Check if output contains an error field
                if let Some(error_val) = actual.get("error") {
                    if test.expect_error {
                        let error_str = error_val.as_str().unwrap_or("");
                        let matches = test
                            .error_contains
                            .as_ref()
                            .is_none_or(|expected| error_str.contains(expected));

                        return TestResult {
                            name: test.name.clone(),
                            passed: matches,
                            duration,
                            error: if matches {
                                None
                            } else {
                                Some(format!(
                                    "Expected error containing '{}', got: {}",
                                    test.error_contains.as_deref().unwrap_or(""),
                                    error_str
                                ))
                            },
                            actual_output: Some(actual),
                        };
                    } else {
                        return TestResult {
                            name: test.name.clone(),
                            passed: false,
                            duration,
                            error: Some(format!("Unexpected error in output: {}", error_val)),
                            actual_output: Some(actual),
                        };
                    }
                }

                // Verify expected output
                if let Some(ref expected) = test.expected_output
                    && &actual != expected
                {
                    return TestResult {
                        name: test.name.clone(),
                        passed: false,
                        duration,
                        error: Some(format!(
                            "Output mismatch:\nExpected: {}\nActual: {}",
                            serde_json::to_string_pretty(expected).unwrap_or_default(),
                            serde_json::to_string_pretty(&actual).unwrap_or_default()
                        )),
                        actual_output: Some(actual),
                    };
                }

                // Verify expected fields
                if let Some(ref fields) = test.expected_fields {
                    for field in fields {
                        let field_value = get_json_path(&actual, &field.path);

                        if field.exists && field_value.is_none() {
                            return TestResult {
                                name: test.name.clone(),
                                passed: false,
                                duration,
                                error: Some(format!("Missing expected field: {}", field.path)),
                                actual_output: Some(actual),
                            };
                        }

                        if let Some(ref expected_value) = field.value
                            && field_value != Some(expected_value)
                        {
                            return TestResult {
                                name: test.name.clone(),
                                passed: false,
                                duration,
                                error: Some(format!(
                                    "Field '{}' mismatch: expected {:?}, got {:?}",
                                    field.path, expected_value, field_value
                                )),
                                actual_output: Some(actual),
                            };
                        }
                    }
                }

                TestResult {
                    name: test.name.clone(),
                    passed: true,
                    duration,
                    error: None,
                    actual_output: Some(actual),
                }
            }
        }
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
