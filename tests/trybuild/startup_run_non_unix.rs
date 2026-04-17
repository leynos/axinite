//! Compile contract for the non-Unix `setup_runtime_management` surface.

use std::{sync::Arc, time::Duration};

use ironclaw::{
    app::AppComponents,
    config::Config,
    context::ContextManager,
    orchestrator::{ReaperConfig, SandboxReaper},
};

fn setup_runtime_management(
    components: &AppComponents,
    config: &Config,
    container_job_manager: &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
) -> tokio::sync::broadcast::Sender<()> {
    let reaper_context_manager = Arc::clone(&components.context_manager);
    maybe_spawn_sandbox_reaper(container_job_manager, reaper_context_manager, config);

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    shutdown_tx
}

fn maybe_spawn_sandbox_reaper(
    container_job_manager: &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    reaper_context_manager: Arc<ContextManager>,
    config: &Config,
) {
    if let Some(jm) = container_job_manager {
        let reaper_jm = Arc::clone(jm);
        let reaper_config = ReaperConfig {
            scan_interval: Duration::from_secs(config.sandbox.reaper_interval_secs),
            orphan_threshold: Duration::from_secs(config.sandbox.orphan_threshold_secs),
            ..ReaperConfig::default()
        };
        let reaper_ctx = Arc::clone(&reaper_context_manager);
        tokio::spawn(async move {
            match SandboxReaper::new(reaper_jm, reaper_ctx, reaper_config).await {
                Ok(reaper) => reaper.run().await,
                Err(e) => tracing::error!("Sandbox reaper failed to initialize: {e}"),
            }
        });
    }
}

fn assert_non_unix_surface(
    components: &AppComponents,
    config: &Config,
    container_job_manager: &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
) {
    let _ = setup_runtime_management(components, config, container_job_manager);
}

fn main() {
    let _ = assert_non_unix_surface
        as fn(
            &AppComponents,
            &Config,
            &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
        );
}
