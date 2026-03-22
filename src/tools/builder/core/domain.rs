//! Core builder domain types and execution contracts.
//!
//! This module defines the structured requirement, result, and configuration
//! types shared across the builder pipeline. It also centralizes small
//! validated domain values, such as [`ProjectName`], so builder call sites do
//! not pass unchecked strings through path joins or command planning.

use super::*;
use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use serde::de::Error as _;

/// Validated identifier used for builder project directories and artifact names.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ProjectName(String);

impl ProjectName {
    /// Validate and construct a project name.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() {
            return Err("project name must not be empty".to_string());
        }
        if !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            return Err(
                "project name must contain only ASCII letters, digits, '-' or '_'".to_string(),
            );
        }
        Ok(Self(value))
    }

    /// Access the validated name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ProjectName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ProjectName {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProjectName> for String {
    fn from(value: ProjectName) -> Self {
        value.0
    }
}

impl<'de> Deserialize<'de> for ProjectName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Explicit command plan for a build or test step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionCommand {
    /// Working directory for the command.
    pub cwd: PathBuf,
    /// Program to execute.
    pub program: String,
    /// Command-line arguments, in order.
    pub args: Vec<String>,
}

/// Requirement specification for building software.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRequirement {
    /// Name for the software.
    pub name: ProjectName,
    /// Description of what it should do.
    pub description: String,
    /// Type of software to build.
    pub software_type: SoftwareType,
    /// Target language/runtime.
    pub language: Language,
    /// Expected input format (for tools/CLIs).
    pub input_spec: Option<String>,
    /// Expected output format.
    pub output_spec: Option<String>,
    /// External dependencies needed.
    pub dependencies: Vec<String>,
    /// Security/capability requirements (for WASM tools).
    pub capabilities: Vec<String>,
}

/// Type of software being built.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareType {
    /// A WASM tool for the agent.
    WasmTool,
    /// A standalone CLI application.
    CliBinary,
    /// A library/crate.
    Library,
    /// A script (Python, Bash, etc.).
    Script,
    /// A web service/API.
    WebService,
}

/// Programming language for the build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
    Bash,
}

impl Language {
    /// Get the file extension for this language.
    pub fn extension(&self) -> &'static str {
        match self {
            Language::Rust => "rs",
            Language::Python => "py",
            Language::TypeScript => "ts",
            Language::JavaScript => "js",
            Language::Go => "go",
            Language::Bash => "sh",
        }
    }

    /// Get the build command for this language.
    pub fn build_command(&self, project_dir: &Path) -> Option<ExecutionCommand> {
        match self {
            Language::Rust => Some(ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "cargo".to_string(),
                args: vec!["build".to_string(), "--release".to_string()],
            }),
            Language::TypeScript => Some(ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "npm".to_string(),
                args: vec!["run".to_string(), "build".to_string()],
            }),
            Language::Go => Some(ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "go".to_string(),
                args: vec!["build".to_string(), "./...".to_string()],
            }),
            Language::Python | Language::JavaScript | Language::Bash => None, // Interpreted
        }
    }

    /// Get the test command for this language.
    pub fn test_command(&self, project_dir: &Path) -> ExecutionCommand {
        match self {
            Language::Rust => ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "cargo".to_string(),
                args: vec!["test".to_string()],
            },
            Language::Python => ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "python".to_string(),
                args: vec!["-m".to_string(), "pytest".to_string()],
            },
            Language::TypeScript | Language::JavaScript => ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "npm".to_string(),
                args: vec!["test".to_string()],
            },
            Language::Go => ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "go".to_string(),
                args: vec!["test".to_string(), "./...".to_string()],
            },
            Language::Bash => ExecutionCommand {
                cwd: project_dir.to_path_buf(),
                program: "sh".to_string(),
                args: vec!["-c".to_string(), "shellcheck *.sh".to_string()],
            },
        }
    }
}

/// Result of a build operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    /// Unique ID for this build.
    pub build_id: Uuid,
    /// The requirement that was built.
    pub requirement: BuildRequirement,
    /// Path to the output artifact.
    pub artifact_path: PathBuf,
    /// Build logs.
    pub logs: Vec<BuildLog>,
    /// Whether the build succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
    /// When the build started.
    pub started_at: DateTime<Utc>,
    /// When the build completed.
    pub completed_at: DateTime<Utc>,
    /// Number of iterations to complete.
    pub iterations: u32,
    /// Validation warnings (for WASM tools).
    #[serde(default)]
    pub validation_warnings: Vec<String>,
    /// Test results summary.
    #[serde(default)]
    pub tests_passed: u32,
    /// Number of tests that failed.
    #[serde(default)]
    pub tests_failed: u32,
    /// Whether the tool was auto-registered (for WASM tools).
    #[serde(default)]
    pub registered: bool,
}

/// A log entry from the build process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildLog {
    pub timestamp: DateTime<Utc>,
    pub phase: BuildPhase,
    pub message: String,
    pub details: Option<String>,
}

/// Phases of the build process.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BuildPhase {
    Analyzing,
    Scaffolding,
    Implementing,
    Building,
    Testing,
    Fixing,
    Validating,
    Registering,
    Packaging,
    Complete,
    Failed,
}

/// Configuration for the software builder.
#[derive(Debug, Clone)]
pub struct BuilderConfig {
    /// Directory where builds happen.
    pub build_dir: PathBuf,
    /// Maximum iterations before giving up.
    pub max_iterations: u32,
    /// Timeout for the entire build.
    pub timeout: Duration,
    /// Whether to clean up failed builds.
    pub cleanup_on_failure: bool,
    /// Whether to validate WASM tools after building.
    pub validate_wasm: bool,
    /// Whether to run tests after building.
    pub run_tests: bool,
    /// Whether to auto-register successful WASM tool builds.
    pub auto_register: bool,
    /// Directory to copy successful WASM tools for persistence.
    pub wasm_output_dir: Option<PathBuf>,
}

impl Default for BuilderConfig {
    fn default() -> Self {
        Self {
            build_dir: std::env::temp_dir().join("ironclaw-builds"),
            max_iterations: 10,
            timeout: Duration::from_secs(600), // 10 minutes
            cleanup_on_failure: false,         // Keep for debugging
            validate_wasm: true,
            run_tests: true,
            auto_register: true,
            wasm_output_dir: None,
        }
    }
}

/// Boxed future used at the dyn-backed builder boundary.
pub type SoftwareBuilderFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Dyn-facing trait for building software behind erased builder handles.
///
/// Callers that need dynamic dispatch store builders as
/// `Arc<dyn SoftwareBuilder>` and pay the boxed-future cost at this boundary.
/// Concrete implementations should normally implement
/// [`NativeSoftwareBuilder`] instead; the blanket adapter below bridges those
/// native futures into this boxed-future surface.
pub trait SoftwareBuilder: Send + Sync {
    /// Analyze a natural language description and extract a structured requirement.
    fn analyze<'a>(
        &'a self,
        description: &'a str,
    ) -> SoftwareBuilderFuture<'a, Result<BuildRequirement, AgentToolError>>;

    /// Build software from a requirement.
    fn build<'a>(
        &'a self,
        requirement: &'a BuildRequirement,
    ) -> SoftwareBuilderFuture<'a, Result<BuildResult, AgentToolError>>;

    /// Attempt to repair a failed build.
    fn repair<'a>(
        &'a self,
        result: &'a BuildResult,
        error: &'a str,
    ) -> SoftwareBuilderFuture<'a, Result<BuildResult, AgentToolError>>;
}

/// Native async sibling trait for concrete builder implementations.
///
/// Concrete builders should implement this trait with ordinary `async fn`
/// methods. The blanket `impl<T> SoftwareBuilder for T` below automatically
/// grants those implementors the dyn-facing [`SoftwareBuilder`] interface by
/// boxing the returned futures only at the dispatch boundary.
///
/// [`super::builder_impl`] follows this pattern for [`LlmSoftwareBuilder`]:
/// `LlmSoftwareBuilder` implements `NativeSoftwareBuilder`, while callers that
/// need dynamic dispatch continue to use `SoftwareBuilder`.
pub trait NativeSoftwareBuilder: Send + Sync {
    /// Analyze a natural language description and extract a structured requirement.
    fn analyze<'a>(
        &'a self,
        description: &'a str,
    ) -> impl Future<Output = Result<BuildRequirement, AgentToolError>> + Send + 'a;

    /// Build software from a requirement.
    fn build<'a>(
        &'a self,
        requirement: &'a BuildRequirement,
    ) -> impl Future<Output = Result<BuildResult, AgentToolError>> + Send + 'a;

    /// Attempt to repair a failed build.
    fn repair<'a>(
        &'a self,
        result: &'a BuildResult,
        error: &'a str,
    ) -> impl Future<Output = Result<BuildResult, AgentToolError>> + Send + 'a;
}

impl<T> SoftwareBuilder for T
where
    T: NativeSoftwareBuilder + Send + Sync,
{
    fn analyze<'a>(
        &'a self,
        description: &'a str,
    ) -> SoftwareBuilderFuture<'a, Result<BuildRequirement, AgentToolError>> {
        Box::pin(NativeSoftwareBuilder::analyze(self, description))
    }

    fn build<'a>(
        &'a self,
        requirement: &'a BuildRequirement,
    ) -> SoftwareBuilderFuture<'a, Result<BuildResult, AgentToolError>> {
        Box::pin(NativeSoftwareBuilder::build(self, requirement))
    }

    fn repair<'a>(
        &'a self,
        result: &'a BuildResult,
        error: &'a str,
    ) -> SoftwareBuilderFuture<'a, Result<BuildResult, AgentToolError>> {
        Box::pin(NativeSoftwareBuilder::repair(self, result, error))
    }
}
