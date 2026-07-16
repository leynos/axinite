//! Steps 8 and 9: Docker and Claude Code sandbox checks, plus heartbeat
//! configuration.

use super::*;

/// Explain why Docker is unavailable and how to remedy it.
fn print_docker_unavailable_hints(
    detection: &crate::sandbox::detect::DockerDetection,
    not_installed: bool,
) {
    println!();
    if not_installed {
        print_error("Docker is not installed.");
        print_info(detection.platform.install_hint());
    } else {
        print_error("Docker is installed but not running.");
        print_info(detection.platform.start_hint());
    }
    println!();
}

/// Report whether an Anthropic API key is available.
///
/// Uses `optional_env()`, which reads both real environment variables and
/// the injected overlay (secrets DB, wizard-set values).
fn has_anthropic_api_key() -> bool {
    crate::config::helpers::optional_env(crate::config::helpers::EnvKey("ANTHROPIC_API_KEY"))
        .ok()
        .flatten()
        .is_some_and(|v| !v.is_empty() && v != OAUTH_PLACEHOLDER)
}

/// Report whether an Anthropic OAuth token is available from `claude login`
/// credentials or the environment overlay.
fn has_anthropic_oauth_token() -> bool {
    if crate::config::ClaudeCodeConfig::extract_oauth_token().is_some() {
        return true;
    }
    crate::config::helpers::optional_env(crate::config::helpers::EnvKey("ANTHROPIC_OAUTH_TOKEN"))
        .ok()
        .flatten()
        .is_some_and(|v| !v.is_empty())
}

/// Report whether any Anthropic credential (API key or OAuth token) is
/// present.
fn anthropic_credentials_present() -> bool {
    has_anthropic_api_key() || has_anthropic_oauth_token()
}

impl SetupWizard {
    /// Step 8: Docker Sandbox -- check Docker installation and availability.
    pub(super) async fn step_docker_sandbox(&mut self) -> Result<(), SetupError> {
        print_info("IronClaw can execute code, run builds, and use tools inside Docker");
        print_info("containers. This keeps your system safe -- commands from the LLM run");
        print_info("in an isolated sandbox with no access to your credentials, limited");
        print_info("filesystem access, and network traffic restricted to an allowlist.");
        println!();
        print_info("Without Docker, code execution tools (shell, file write) run directly");
        print_info("on your machine with no isolation.");
        println!();

        if !confirm("Enable Docker sandbox?", false).map_err(SetupError::Io)? {
            self.settings.sandbox.enabled = false;
            print_info("Sandbox disabled. You can enable it later with SANDBOX_ENABLED=true.");
            return Ok(());
        }

        // Check Docker availability
        let detection = crate::sandbox::detect::check_docker().await;

        match detection.status {
            crate::sandbox::detect::DockerStatus::Available => {
                self.settings.sandbox.enabled = true;
                print_success("Docker is installed and running. Sandbox enabled.");
            }
            crate::sandbox::detect::DockerStatus::NotInstalled
            | crate::sandbox::detect::DockerStatus::NotRunning => {
                let not_installed =
                    detection.status == crate::sandbox::detect::DockerStatus::NotInstalled;
                print_docker_unavailable_hints(&detection, not_installed);
                self.offer_docker_retry(not_installed).await?;
            }
            crate::sandbox::detect::DockerStatus::Disabled => {
                self.settings.sandbox.enabled = false;
            }
        }

        // Claude Code sandbox sub-step (only if Docker sandbox is enabled)
        if self.settings.sandbox.enabled {
            self.step_claude_code_sandbox().await?;
        }

        Ok(())
    }

    /// Offer to re-check Docker after the user installs or starts it,
    /// enabling the sandbox when the retry succeeds.
    async fn offer_docker_retry(&mut self, not_installed: bool) -> Result<(), SetupError> {
        let retry_prompt = if not_installed {
            "Retry after installing Docker?"
        } else {
            "Retry after starting Docker?"
        };
        if !confirm(retry_prompt, false).map_err(SetupError::Io)? {
            self.decline_docker_retry(not_installed);
            return Ok(());
        }

        let retry = crate::sandbox::detect::check_docker().await;
        self.apply_docker_retry_outcome(retry.status.is_ok(), not_installed);
        Ok(())
    }

    /// Disable the sandbox after the user declines the Docker retry.
    fn decline_docker_retry(&mut self, not_installed: bool) {
        self.settings.sandbox.enabled = false;
        print_info(if not_installed {
            "Sandbox disabled. Install Docker and set SANDBOX_ENABLED=true later."
        } else {
            "Sandbox disabled. Start Docker and set SANDBOX_ENABLED=true later."
        });
    }

    /// Record the Docker retry result, enabling the sandbox when Docker is
    /// now available.
    fn apply_docker_retry_outcome(&mut self, available: bool, not_installed: bool) {
        self.settings.sandbox.enabled = available;
        if available {
            print_success(if not_installed {
                "Docker is now available. Sandbox enabled."
            } else {
                "Docker is now running. Sandbox enabled."
            });
        } else {
            print_info(if not_installed {
                "Docker still not available. Sandbox disabled for now."
            } else {
                "Docker still not responding. Sandbox disabled for now."
            });
        }
    }

    /// Claude Code sandbox sub-step: enable Claude CLI inside Docker containers.
    async fn step_claude_code_sandbox(&mut self) -> Result<(), SetupError> {
        println!();
        print_info("Claude Code mode lets the agent delegate complex tasks to Claude CLI");
        print_info("running inside sandboxed Docker containers.");
        println!();

        if !confirm("Enable Claude Code sandbox mode?", false).map_err(SetupError::Io)? {
            self.settings.sandbox.claude_code_enabled = false;
            return Ok(());
        }

        if anthropic_credentials_present() {
            self.enable_claude_code();
            return Ok(());
        }

        print_error("No Anthropic credentials found.");
        print_info("Claude Code needs ANTHROPIC_API_KEY or an OAuth token from `claude login`.");
        println!();

        if !confirm("Retry after setting up credentials?", false).map_err(SetupError::Io)? {
            self.settings.sandbox.claude_code_enabled = false;
            print_info("Claude Code disabled. Enable with CLAUDE_CODE_ENABLED=true later.");
            return Ok(());
        }

        self.retry_claude_code_credentials();
        Ok(())
    }

    /// Enable Claude Code sandbox mode and report success.
    fn enable_claude_code(&mut self) {
        self.settings.sandbox.claude_code_enabled = true;
        print_success("Claude Code sandbox enabled");
    }

    /// Re-check credentials after the user set them up, enabling Claude Code
    /// only when they are now present.
    fn retry_claude_code_credentials(&mut self) {
        if anthropic_credentials_present() {
            self.enable_claude_code();
            return;
        }
        self.settings.sandbox.claude_code_enabled = false;
        print_info("No credentials found. Claude Code disabled for now.");
        print_info("Set ANTHROPIC_API_KEY or run `claude login` and enable later.");
    }

    /// Step 9: Heartbeat configuration.
    pub(super) fn step_heartbeat(&mut self) -> Result<(), SetupError> {
        print_info("Heartbeat runs periodic background tasks (e.g., checking your calendar,");
        print_info("monitoring for notifications, running scheduled workflows).");
        println!();

        if !confirm("Enable heartbeat?", false).map_err(SetupError::Io)? {
            self.settings.heartbeat.enabled = false;
            print_info("Heartbeat disabled.");
            return Ok(());
        }

        self.settings.heartbeat.enabled = true;

        // Interval
        let interval_str = optional_input("Check interval in minutes", Some("default: 30"))
            .map_err(SetupError::Io)?;

        if let Some(s) = interval_str {
            if let Ok(mins) = s.parse::<u64>() {
                self.settings.heartbeat.interval_secs = mins * 60;
            }
        } else {
            self.settings.heartbeat.interval_secs = 1800; // 30 minutes
        }

        // Notify channel
        let notify_channel = optional_input("Notify channel on findings", Some("e.g., telegram"))
            .map_err(SetupError::Io)?;
        self.settings.heartbeat.notify_channel = notify_channel;

        print_success(&format!(
            "Heartbeat enabled (every {} minutes)",
            self.settings.heartbeat.interval_secs / 60
        ));

        Ok(())
    }
}
