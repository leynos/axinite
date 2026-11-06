//! Job and project-file DTOs for the web gateway API.

use serde::Serialize;
use uuid::Uuid;

// --- Jobs ---

/// One row in the jobs list, covering both sandbox and agent jobs.
#[derive(Debug, Serialize)]
pub struct JobInfo {
    /// Job UUID, unique across sandbox and agent jobs.
    pub id: Uuid,
    /// Job title. For sandbox jobs this is the task text.
    pub title: String,
    /// UI state. Sandbox `creating` and `running` appear as `pending` and `in_progress`;
    /// other values pass through unchanged.
    pub state: String,
    /// Owning user's ID.
    pub user_id: String,
    /// Creation time as an RFC 3339 timestamp.
    pub created_at: String,
    /// Start time as an RFC 3339 timestamp, or `None` if the job has not started.
    pub started_at: Option<String>,
}

/// Response body for the job list endpoint.
#[derive(Debug, Serialize)]
pub struct JobListResponse {
    /// Sandbox jobs first, followed by agent jobs not already listed as sandbox jobs.
    pub jobs: Vec<JobInfo>,
}

/// Aggregate job counts across sandbox and agent jobs.
#[derive(Debug, Serialize)]
pub struct JobSummaryResponse {
    /// Total jobs of both kinds.
    pub total: usize,
    /// Jobs not yet started: sandbox `creating` and agent `pending`.
    pub pending: usize,
    /// Jobs currently running.
    pub in_progress: usize,
    /// Jobs that finished successfully.
    pub completed: usize,
    /// Failed jobs, including sandbox jobs interrupted by a restart.
    pub failed: usize,
    /// Agent jobs flagged as stuck. Sandbox jobs are never counted here.
    pub stuck: usize,
}

/// Full detail for one job, returned by the job detail endpoint.
#[derive(Debug, Serialize)]
pub struct JobDetailResponse {
    /// Job UUID.
    pub id: Uuid,
    /// Job title. For sandbox jobs this is the task text.
    pub title: String,
    /// Job description. Empty for sandbox jobs, which carry only a title.
    pub description: String,
    /// UI state, mapped as in [`JobInfo::state`].
    pub state: String,
    /// Owning user's ID.
    pub user_id: String,
    /// Creation time as an RFC 3339 timestamp.
    pub created_at: String,
    /// Start time as an RFC 3339 timestamp, or `None` if the job has not started.
    pub started_at: Option<String>,
    /// Completion time as an RFC 3339 timestamp, or `None` while the job is unfinished.
    pub completed_at: Option<String>,
    /// Seconds from start to completion, or to now while the job runs. `None` if never started.
    pub elapsed_secs: Option<u64>,
    /// Host directory backing a sandbox job. Omitted for agent jobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    /// Web path for browsing the project, such as `/projects/<id>/`. Omitted for agent jobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browse_url: Option<String>,
    /// Non-default sandbox mode, currently `claude_code`. Omitted for worker and agent jobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_mode: Option<String>,
    /// Recorded state changes in chronological order. Empty for agent jobs.
    pub transitions: Vec<TransitionInfo>,
    /// Whether this job can be restarted from the UI.
    #[serde(default)]
    pub can_restart: bool,
    /// Whether follow-up prompts can be sent to this job.
    #[serde(default)]
    pub can_prompt: bool,
    /// The kind of job: "sandbox" or "agent".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_kind: Option<String>,
}

// --- Project Files ---

/// One entry in a job's project directory listing.
#[derive(Debug, Serialize)]
pub struct ProjectFileEntry {
    /// File or directory name, without its parent path.
    pub name: String,
    /// Path relative to the project root, with `/` separators.
    pub path: String,
    /// True when the entry is a directory.
    pub is_dir: bool,
}

/// Directory listing for a job's project files.
#[derive(Debug, Serialize)]
pub struct ProjectFilesResponse {
    /// Directories first, then files, each group sorted by name.
    pub entries: Vec<ProjectFileEntry>,
}

/// Contents of one project file.
#[derive(Debug, Serialize)]
pub struct ProjectFileReadResponse {
    /// The requested path, relative to the project root.
    pub path: String,
    /// Full file contents, read as UTF-8 text.
    pub content: String,
}

/// One recorded state change for a sandbox job.
#[derive(Debug, Serialize)]
pub struct TransitionInfo {
    /// State before the transition. Sandbox statuses are reported unmapped.
    pub from: String,
    /// State after the transition.
    pub to: String,
    /// When the transition happened, as an RFC 3339 timestamp.
    pub timestamp: String,
    /// Failure reason recorded with the completion transition, if any. `None` for the start.
    pub reason: Option<String>,
}
