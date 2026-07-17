//! External dependency checks: service installation, the Docker daemon,
//! and external binaries on the PATH.

use super::CheckResult;

// ── Service ─────────────────────────────────────────────────

pub(super) fn check_service_installed() -> CheckResult {
    if cfg!(target_os = "macos") {
        let plist =
            dirs::home_dir().map(|h| h.join("Library/LaunchAgents/com.axinite.daemon.plist"));
        service_unit_result(plist, "launchd plist")
    } else if cfg!(target_os = "linux") {
        let unit = dirs::home_dir().map(|h| h.join(".config/systemd/user/axinite.service"));
        service_unit_result(unit, "systemd unit")
    } else {
        CheckResult::Skip("service management not supported on this platform".into())
    }
}

/// Report whether a service definition exists at the platform's expected path.
fn service_unit_result(path: Option<std::path::PathBuf>, label: &str) -> CheckResult {
    match path {
        Some(path) if path.exists() => {
            CheckResult::Pass(format!("{label} installed ({})", path.display()))
        }
        Some(_) => CheckResult::Skip("not installed (run `axinite service install`)".into()),
        None => CheckResult::Skip("cannot determine home directory".into()),
    }
}

// ── Docker daemon ───────────────────────────────────────────

pub(super) async fn check_docker_daemon() -> CheckResult {
    let detection = crate::sandbox::check_docker().await;
    match detection.status {
        crate::sandbox::DockerStatus::Available => CheckResult::Pass("running".into()),
        crate::sandbox::DockerStatus::NotInstalled => CheckResult::Skip(format!(
            "not installed. {}",
            detection.platform.install_hint()
        )),
        crate::sandbox::DockerStatus::NotRunning => CheckResult::Fail(format!(
            "installed but not running. {}",
            detection.platform.start_hint()
        )),
        crate::sandbox::DockerStatus::Disabled => CheckResult::Skip("sandbox disabled".into()),
    }
}

// ── External binary ─────────────────────────────────────────

pub(super) fn check_binary(name: &str, args: &[&str]) -> CheckResult {
    match std::process::Command::new(name)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                CheckResult::Pass(extract_version_line(&output))
            } else {
                CheckResult::Fail(format!("exited with {}", output.status))
            }
        }
        Err(_) => CheckResult::Skip(format!("{name} not found in PATH")),
    }
}

/// Extract the first version line from a tool's output, preferring stdout.
///
/// Some tools print their version to stderr, so fall back to it when
/// stdout is empty.
fn extract_version_line(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        stderr.trim().lines().next().unwrap_or("").to_string()
    } else {
        stdout.lines().next().unwrap_or("").to_string()
    }
}
