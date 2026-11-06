//! Registration settings for the built-in tool families.

use super::*;

/// Settings for the image generation and editing tools.
pub struct ImageToolsRegistration {
    /// Base URL of the image API. Generation posts to `{base}/v1/images/generations`.
    pub api_base_url: String,
    /// API key sent with each image request.
    pub api_key: String,
    /// Model name sent to the generation endpoint.
    pub gen_model: String,
    /// Directory that image input paths must stay inside, when set.
    pub base_dir: Option<std::path::PathBuf>,
}

impl ImageToolsRegistration {
    /// Create a registration with `base_dir` unset.
    pub fn new(api_base_url: String, api_key: String, gen_model: String) -> Self {
        Self {
            api_base_url,
            api_key,
            gen_model,
            base_dir: None,
        }
    }
}

/// Settings for the vision (image analysis) tool.
pub struct VisionToolsRegistration {
    /// Base URL of the vision model API.
    pub api_base_url: String,
    /// API key sent with each analysis request.
    pub api_key: String,
    /// Model used to analyse images.
    pub vision_model: String,
    /// Directory that image input paths must stay inside, when set.
    pub base_dir: Option<std::path::PathBuf>,
}

/// Shared handles that the job-management tools are wired to.
pub struct RegisterJobToolsOptions {
    /// Context store shared by every job tool.
    pub context_manager: Arc<ContextManager>,
    /// Scheduler handle, filled after startup, used to dispatch local jobs.
    pub scheduler_slot: Option<crate::tools::builtin::SchedulerSlot>,
    /// Sandbox job manager. When set, `create_job` runs jobs in Docker containers.
    pub job_manager: Option<Arc<ContainerJobManager>>,
    /// Database for persisted job data. When set, `job_events` is registered.
    pub store: Option<Arc<dyn Database>>,
    /// Broadcast sender for job events. Monitoring needs this and `inject_tx` together.
    pub job_event_tx: Option<tokio::sync::broadcast::Sender<(uuid::Uuid, SseEvent)>>,
    /// Feeds monitored job output back into the main agent loop.
    pub inject_tx: Option<tokio::sync::mpsc::Sender<IncomingMessage>>,
    /// Queue of follow-up prompts for running sandbox jobs. When set, `job_prompt` is registered.
    pub prompt_queue: Option<PromptQueue>,
    /// Secrets store that `create_job` uses to validate credentials.
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}
