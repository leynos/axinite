//! Docker connection discovery and feature-gated fallbacks.
//!
//! This module centralises Docker daemon discovery for sandbox execution. With
//! the `docker` feature enabled it probes the default client settings first and
//! then checks user-owned Unix sockets in this order:
//! `~/.docker/run/docker.sock`, `~/.colima/default/docker.sock`,
//! `~/.rd/docker.sock`, `$XDG_RUNTIME_DIR/docker.sock`, and
//! `/run/user/<uid>/docker.sock`. Without the feature it returns a clear
//! `DockerNotAvailable` error immediately so callers can degrade gracefully.
#[cfg(all(unix, any(feature = "docker", test)))]
use std::path::PathBuf;

use super::*;

/// Connect to the Docker daemon.
///
/// When the crate is compiled without the `docker` feature, this returns
/// `SandboxError::DockerNotAvailable` immediately via
/// `docker_feature_disabled_error()`.
///
/// With the `docker` feature enabled, this delegates to
/// `connect_docker_inner()` and tries these locations in order:
///
/// 1. `DOCKER_HOST` env var (bollard default)
/// 2. `/var/run/docker.sock` (Linux default; also used by OrbStack and Podman
///    Desktop on macOS)
/// 3. `~/.docker/run/docker.sock` (Docker Desktop 4.13+ on macOS — primary
///    user-owned socket)
/// 4. `~/.colima/default/docker.sock` (Colima — popular lightweight Docker
///    Desktop alternative)
/// 5. `~/.rd/docker.sock` (Rancher Desktop on macOS)
/// 6. `$XDG_RUNTIME_DIR/docker.sock` (common rootless Docker socket on Linux)
/// 7. `/run/user/$UID/docker.sock` (rootless Docker fallback on Linux)
pub async fn connect_docker() -> Result<DockerConnection> {
    #[cfg(feature = "docker")]
    {
        connect_docker_inner().await
    }

    #[cfg(not(feature = "docker"))]
    {
        Err(docker_feature_disabled_error())
    }
}

#[cfg(feature = "docker")]
async fn connect_docker_inner() -> Result<DockerConnection> {
    // First try bollard defaults (checks DOCKER_HOST env var, then /var/run/docker.sock).
    // This covers Linux, OrbStack (updates the /var/run symlink), and any user with
    // DOCKER_HOST set to their runtime's socket.
    if let Ok(docker) = DockerConnection::connect_with_local_defaults()
        && docker.ping().await.is_ok()
    {
        return Ok(docker);
    }

    #[cfg(unix)]
    {
        // Try well-known user-owned socket locations for desktop and rootless runtimes.
        // Docker Desktop 4.13+ (stabilised in 4.18) stopped creating the
        // /var/run/docker.sock symlink by default and moved the API socket
        // to ~/.docker/run/docker.sock.
        for sock in unix_socket_candidates() {
            if sock.exists() {
                let sock_str = sock.to_string_lossy();
                if let Ok(docker) = DockerConnection::connect_with_socket(
                    &sock_str,
                    120,
                    bollard::API_DEFAULT_VERSION,
                ) && docker.ping().await.is_ok()
                {
                    return Ok(docker);
                }
            }
        }
    }

    Err(SandboxError::DockerNotAvailable {
        reason: concat!(
            "Could not connect to Docker daemon. Tried: $DOCKER_HOST, ",
            "/var/run/docker.sock, ",
            "~/.docker/run/docker.sock, ",
            "~/.colima/default/docker.sock, ",
            "~/.rd/docker.sock, ",
            "$XDG_RUNTIME_DIR/docker.sock, ",
            "/run/user/$UID/docker.sock"
        )
        .to_string(),
    })
}

#[cfg(not(feature = "docker"))]
pub(crate) fn docker_feature_disabled_error() -> SandboxError {
    SandboxError::DockerNotAvailable {
        reason: DOCKER_FEATURE_DISABLED_REASON.to_string(),
    }
}

#[cfg(all(unix, feature = "docker"))]
fn unix_socket_candidates() -> Vec<PathBuf> {
    unix_socket_candidates_from_env(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        Some({
            // SAFETY: `geteuid` has no preconditions and simply returns the
            // effective user ID for the current process.
            unsafe { libc::geteuid() }.to_string()
        }),
    )
}

#[cfg(all(unix, any(feature = "docker", test)))]
fn unix_socket_candidates_from_env(
    home: Option<PathBuf>,
    xdg_runtime_dir: Option<PathBuf>,
    uid: Option<String>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut push_unique = |path: PathBuf| {
        if !candidates.iter().any(|existing| existing == &path) {
            candidates.push(path);
        }
    };

    if let Some(home) = home {
        push_unique(home.join(".docker/run/docker.sock")); // Docker Desktop 4.13+
        push_unique(home.join(".colima/default/docker.sock")); // Colima
        push_unique(home.join(".rd/docker.sock")); // Rancher Desktop
    }

    if let Some(xdg_runtime_dir) = xdg_runtime_dir {
        push_unique(xdg_runtime_dir.join("docker.sock"));
    }

    if let Some(uid) = uid.filter(|value| !value.is_empty()) {
        push_unique(PathBuf::from(format!("/run/user/{uid}/docker.sock")));
    }

    candidates
}

/// Check whether the Docker daemon is responsive.
///
/// This asynchronous helper pings the supplied [`DockerConnection`] and
/// returns `true` when the daemon responds successfully.
///
/// When compiled with the `docker` feature, this uses
/// `DockerConnection::ping()` and returns `true` on `Ok(_)`. Without the
/// `docker` feature, it always returns `false`.
pub async fn docker_is_responsive(docker: &DockerConnection) -> bool {
    #[cfg(feature = "docker")]
    {
        docker.ping().await.is_ok()
    }

    #[cfg(not(feature = "docker"))]
    {
        let _ = docker;
        false
    }
}

/// Ensure that the Docker daemon is responsive.
///
/// This asynchronous helper pings the supplied [`DockerConnection`] and
/// returns `Ok(())` when the daemon responds.
///
/// When compiled with the `docker` feature, ping failures are mapped to
/// [`SandboxError::DockerNotAvailable`]. Without the `docker` feature, this
/// returns `docker_feature_disabled_error()` immediately. The function has no
/// side effects and does not panic.
pub async fn ensure_docker_responsive(docker: &DockerConnection) -> Result<()> {
    #[cfg(feature = "docker")]
    {
        docker
            .ping()
            .await
            .map(|_| ())
            .map_err(|e| SandboxError::DockerNotAvailable {
                reason: e.to_string(),
            })
    }

    #[cfg(not(feature = "docker"))]
    {
        let _ = docker;
        Err(docker_feature_disabled_error())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::PathBuf;

    use super::unix_socket_candidates_from_env;

    #[test]
    fn test_unix_socket_candidates_include_rootless_paths() {
        let candidates = unix_socket_candidates_from_env(
            Some(PathBuf::from("/home/tester")),
            Some(PathBuf::from("/run/user/1000")),
            Some("1000".to_string()),
        );

        assert!(candidates.contains(&PathBuf::from("/home/tester/.docker/run/docker.sock")));
        assert!(candidates.contains(&PathBuf::from("/home/tester/.colima/default/docker.sock")));
        assert!(candidates.contains(&PathBuf::from("/home/tester/.rd/docker.sock")));
        assert!(candidates.contains(&PathBuf::from("/run/user/1000/docker.sock")));
    }
}
