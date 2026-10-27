//! Interpret the deployment control for the Docker restart loop.

use super::EnvContext;

impl EnvContext {
    /// Whether this snapshot enables the Docker entrypoint restart loop.
    ///
    /// Production callers must use [`Self::capture_process`] for this
    /// deployment control so dotenv files cannot enable a process exit.
    pub fn is_docker_restart_environment(&self) -> bool {
        self.get("AXINITE_IN_DOCKER")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }
}
