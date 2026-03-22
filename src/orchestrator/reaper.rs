//! Orphaned Docker container cleanup.
//!
//! The `SandboxReaper` periodically scans Docker for IronClaw-labelled
//! containers and cleans up those whose corresponding jobs are no longer
//! active.

#[cfg(any(test, feature = "docker"))]
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::context::ContextManager;
use crate::orchestrator::job_manager::ContainerJobManager;
#[cfg(feature = "docker")]
use crate::sandbox::connect_docker;
#[cfg(feature = "docker")]
use crate::sandbox::container::DockerConnection;

/// Configuration for the sandbox reaper.
#[derive(Debug, Clone)]
pub struct ReaperConfig {
    /// How often to scan for orphaned containers.
    pub scan_interval: Duration,
    /// Containers older than this with no active job are reaped.
    pub orphan_threshold: Duration,
    /// Label key for looking up job IDs in Docker metadata.
    pub container_label: String,
}

impl Default for ReaperConfig {
    fn default() -> Self {
        Self {
            scan_interval: Duration::from_secs(300),
            orphan_threshold: Duration::from_secs(600),
            container_label: "ironclaw.job_id".to_string(),
        }
    }
}

/// Background task that periodically cleans up orphaned Docker containers.
pub struct SandboxReaper {
    backend: ReaperBackend,
    job_manager: Arc<ContainerJobManager>,
    context_manager: Arc<ContextManager>,
    config: ReaperConfig,
}

#[derive(Debug, Clone)]
struct ReaperContainer {
    id: String,
    job_id: Uuid,
    created_at: DateTime<Utc>,
}

enum ReaperBackend {
    #[cfg(feature = "docker")]
    Docker(DockerConnection),
    #[cfg(not(feature = "docker"))]
    Noop,
}

impl ReaperBackend {
    async fn list_ironclaw_containers(
        &self,
        label: &str,
    ) -> Result<Vec<ReaperContainer>, crate::sandbox::SandboxError> {
        #[cfg(not(feature = "docker"))]
        let _ = label;

        match self {
            #[cfg(feature = "docker")]
            Self::Docker(docker) => list_docker_containers(docker, label).await,
            #[cfg(not(feature = "docker"))]
            Self::Noop => Ok(Vec::new()),
        }
    }

    async fn reap_container(
        &self,
        container_id: &str,
        job_id: Uuid,
        job_manager: &ContainerJobManager,
    ) {
        #[cfg(not(feature = "docker"))]
        let _ = job_manager;

        match self {
            #[cfg(feature = "docker")]
            Self::Docker(docker) => {
                reap_with_docker(docker, container_id, job_id, job_manager).await;
            }
            #[cfg(not(feature = "docker"))]
            Self::Noop => {
                tracing::warn!(
                    job_id = %job_id,
                    container_id = %container_id,
                    "Skipping reaper cleanup because Docker support was not compiled in"
                );
            }
        }
    }
}

#[cfg(any(test, feature = "docker"))]
fn parse_job_id_label(labels: &HashMap<String, String>, label: &str) -> Option<Uuid> {
    labels
        .get(label)
        .and_then(|value| value.parse::<Uuid>().ok())
}

#[cfg(any(test, feature = "docker"))]
fn parse_created_at_label(
    labels: &HashMap<String, String>,
    created: Option<i64>,
) -> Option<DateTime<Utc>> {
    labels
        .get("ironclaw.created_at")
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .or_else(|| created.and_then(|timestamp| DateTime::from_timestamp(timestamp, 0)))
}

impl SandboxReaper {
    /// Create a new reaper.
    ///
    /// With the `docker` feature enabled this connects to Docker eagerly and
    /// returns an error if the daemon is unavailable. Without that feature it
    /// degrades gracefully to a noop backend.
    pub async fn new(
        job_manager: Arc<ContainerJobManager>,
        context_manager: Arc<ContextManager>,
        config: ReaperConfig,
    ) -> Result<Self, crate::sandbox::SandboxError> {
        #[cfg(feature = "docker")]
        let backend = ReaperBackend::Docker(connect_docker().await?);

        #[cfg(feature = "docker")]
        let _ = job_manager.containers();

        #[cfg(not(feature = "docker"))]
        let backend = ReaperBackend::Noop;

        Ok(Self {
            backend,
            job_manager,
            context_manager,
            config,
        })
    }

    /// Run the reaper loop forever. Should be spawned with `tokio::spawn`.
    pub async fn run(self) {
        if self.config.scan_interval.as_secs() == 0 {
            tracing::error!(
                "Reaper: scan_interval must be > 0, got {:?}. Reaper will not start.",
                self.config.scan_interval
            );
            return;
        }

        let mut interval = tokio::time::interval(self.config.scan_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            self.scan_and_reap().await;
        }
    }

    async fn scan_and_reap(&self) {
        let containers = match self.list_ironclaw_containers().await {
            Ok(containers) => containers,
            Err(e) => {
                tracing::error!(error = %e, "Reaper: failed to list Docker containers");
                return;
            }
        };

        let now = Utc::now();
        let threshold = match chrono::Duration::from_std(self.config.orphan_threshold) {
            Ok(duration) => duration,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Reaper: failed to convert orphan_threshold to chrono::Duration, using default of 10 minutes"
                );
                chrono::Duration::minutes(10)
            }
        };

        for (container_id, job_id, created_at) in containers {
            let age = now.signed_duration_since(created_at);
            if age < threshold {
                continue;
            }

            let is_active = match self.context_manager.get_context(job_id).await {
                Ok(ctx) => ctx.state.is_active(),
                Err(_) => false,
            };
            if is_active {
                tracing::debug!(
                    job_id = %job_id,
                    container_id = %&container_id[..12.min(container_id.len())],
                    "Reaper: container has active job, skipping"
                );
                continue;
            }

            tracing::info!(
                job_id = %job_id,
                container_id = %&container_id[..12.min(container_id.len())],
                age_secs = age.num_seconds(),
                "Reaper: orphaned container detected, cleaning up"
            );

            self.reap_container(&container_id, job_id).await;
        }
    }

    /// List all IronClaw-managed containers from Docker.
    ///
    /// Returns tuples of `(container_id, job_id, created_at)`.
    async fn list_ironclaw_containers(
        &self,
    ) -> Result<Vec<(String, Uuid, DateTime<Utc>)>, crate::sandbox::SandboxError> {
        let items = self
            .backend
            .list_ironclaw_containers(&self.config.container_label)
            .await?;

        Ok(items
            .into_iter()
            .map(|container| (container.id, container.job_id, container.created_at))
            .collect())
    }

    /// Stop and remove a single orphaned container.
    async fn reap_container(&self, container_id: &str, job_id: Uuid) {
        self.backend
            .reap_container(container_id, job_id, &self.job_manager)
            .await;
    }
}

#[cfg(feature = "docker")]
async fn list_docker_containers(
    docker: &DockerConnection,
    label: &str,
) -> Result<Vec<ReaperContainer>, crate::sandbox::SandboxError> {
    use bollard::container::ListContainersOptions;

    let mut filters = HashMap::new();
    filters.insert("label", vec![label]);

    let options = ListContainersOptions {
        all: true,
        filters,
        ..Default::default()
    };

    let summaries = docker.list_containers(Some(options)).await.map_err(|e| {
        crate::sandbox::SandboxError::DockerNotAvailable {
            reason: e.to_string(),
        }
    })?;
    let mut result = Vec::new();

    for summary in summaries {
        let container_id = match summary.id {
            Some(id) => id,
            None => continue,
        };

        let labels = summary.labels.unwrap_or_default();
        let job_id = match parse_job_id_label(&labels, label) {
            Some(id) => id,
            None => {
                tracing::warn!(
                    container_id = %&container_id[..12.min(container_id.len())],
                    label_key = %label,
                    "Reaper: ironclaw container missing valid job_id label"
                );
                continue;
            }
        };

        let created_at = match parse_created_at_label(&labels, summary.created) {
            Some(ts) => ts,
            None => {
                tracing::warn!(
                    container_id = %&container_id[..12.min(container_id.len())],
                    "Reaper: could not determine creation time for container, skipping"
                );
                continue;
            }
        };

        result.push(ReaperContainer {
            id: container_id,
            job_id,
            created_at,
        });
    }

    Ok(result)
}

#[cfg(feature = "docker")]
async fn reap_with_docker(
    docker: &DockerConnection,
    container_id: &str,
    job_id: Uuid,
    job_manager: &ContainerJobManager,
) {
    match job_manager.stop_job(job_id).await {
        Ok(()) => {
            tracing::info!(
                job_id = %job_id,
                "Reaper: cleaned up orphaned container via job_manager"
            );
            return;
        }
        Err(e) => {
            tracing::debug!(
                job_id = %job_id,
                error = %e,
                "Reaper: job_manager.stop_job failed (likely no handle after restart), falling back to direct Docker cleanup"
            );
        }
    }

    if let Err(e) = docker
        .stop_container(
            container_id,
            Some(bollard::container::StopContainerOptions { t: 10 }),
        )
        .await
    {
        tracing::debug!(
            job_id = %job_id,
            container_id = %&container_id[..12.min(container_id.len())],
            error = %e,
            "Reaper: stop_container failed (may already be stopped)"
        );
    }

    if let Err(e) = docker
        .remove_container(
            container_id,
            Some(bollard::container::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
    {
        tracing::error!(
            job_id = %job_id,
            container_id = %&container_id[..12.min(container_id.len())],
            error = %e,
            "Reaper: failed to remove orphaned container"
        );
    } else {
        tracing::info!(
            job_id = %job_id,
            container_id = %&container_id[..12.min(container_id.len())],
            "Reaper: removed orphaned container via direct Docker API"
        );
    }
}

#[cfg(test)]
mod tests;
