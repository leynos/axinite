//! Container orchestrator setup owned by the startup boundary.

use axinite::{
    app::AppComponents,
    config::{Config, EnvContext},
};

use crate::startup::context::OrchestratorContext;

/// Initialize the container manager, job-event sender, prompt queue, and
/// Docker status from startup's resolved environment snapshot.
pub(crate) async fn setup_orchestrator_context(
    config: &Config,
    components: &AppComponents,
    env_context: &EnvContext,
) -> OrchestratorContext {
    let orchestrator = axinite::orchestrator::setup_orchestrator_from_context(
        config,
        env_context,
        &components.llm,
        &components.tools,
        components.db.as_ref(),
        components.secrets_store.as_ref(),
    )
    .await;

    OrchestratorContext {
        container_job_manager: orchestrator.container_job_manager,
        job_event_tx: orchestrator.job_event_tx,
        prompt_queue: orchestrator.prompt_queue,
        docker_status: orchestrator.docker_status,
    }
}
