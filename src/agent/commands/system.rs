//! System slash-command handling: /help, /model, /version, /tools, /debug,
//! /ping, /restart, and legacy command routing from the Router.

use crate::agent::Agent;
use crate::agent::submission::SubmissionResult;
use crate::error::Error;

impl Agent {
    /// Handle system commands that bypass thread-state checks entirely.
    pub(in crate::agent) async fn handle_system_command(
        &self,
        command: &str,
        args: &[String],
        channel: &str,
    ) -> Result<SubmissionResult, Error> {
        match command {
            "help" => Ok(SubmissionResult::response(concat!(
                "System:\n",
                "  /help             Show this help\n",
                "  /model [name]     Show or switch the active model\n",
                "  /version          Show version info\n",
                "  /tools            List available tools\n",
                "  /debug            Toggle debug mode\n",
                "  /ping             Connectivity check\n",
                "\n",
                "Jobs:\n",
                "  /job <desc>       Create a new job\n",
                "  /status [id]      Check job status\n",
                "  /cancel <id>      Cancel a job\n",
                "  /list             List all jobs\n",
                "\n",
                "Session:\n",
                "  /undo             Undo last turn\n",
                "  /redo             Redo undone turn\n",
                "  /compact          Compress context window\n",
                "  /clear            Clear current thread\n",
                "  /interrupt        Stop current operation\n",
                "  /new              New conversation thread\n",
                "  /thread <id>      Switch to thread\n",
                "  /resume <id>      Resume from checkpoint\n",
                "\n",
                "Skills:\n",
                "  /skills             List installed skills\n",
                "  /skills search <q>  Search ClawHub registry\n",
                "\n",
                "Agent:\n",
                "  /heartbeat        Run heartbeat check\n",
                "  /summarize        Summarize current thread\n",
                "  /suggest          Suggest next steps\n",
                "  /restart          Gracefully restart the process\n",
                "\n",
                "  /quit             Exit",
            ))),

            "ping" => Ok(SubmissionResult::response("pong!")),

            "restart" => self.handle_restart(channel).await,

            "version" => Ok(SubmissionResult::response(format!(
                "{} v{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))),

            "tools" => {
                let tools = self.tools().list().await;
                Ok(SubmissionResult::response(format!(
                    "Available tools: {}",
                    tools.join(", ")
                )))
            }

            "debug" => {
                // Debug toggle is handled client-side in the REPL.
                // For non-REPL channels, just acknowledge.
                Ok(SubmissionResult::ok_with_message(
                    "Debug toggle is handled by your client.",
                ))
            }

            "skills" => self.handle_skills_command(args).await,

            "model" => {
                if args.is_empty() {
                    Ok(SubmissionResult::response(self.show_models().await))
                } else {
                    self.switch_model(&args[0]).await
                }
            }

            _ => Ok(SubmissionResult::error(format!(
                "Unknown command: {}. Try /help",
                command
            ))),
        }
    }

    /// Handle `/restart`: gated on the gateway channel and a Docker
    /// environment, then runs the restart tool directly (no LLM planning).
    async fn handle_restart(&self, channel: &str) -> Result<SubmissionResult, Error> {
        tracing::info!("[commands::restart] Restart command received");
        // Channel authorization check: restart is only available via web interface
        if channel != "gateway" {
            tracing::warn!(
                "[commands::restart] Restart rejected: not from gateway channel (from: {})",
                channel
            );
            return Ok(SubmissionResult::error(
                "Restart is only available through the web interface with explicit user confirmation. \
                 Use the Restart button in the UI."
                    .to_string(),
            ));
        }
        // Environment check: restart is only available in Docker containers
        let in_docker = std::env::var("IRONCLAW_IN_DOCKER")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        tracing::debug!("[commands::restart] IRONCLAW_IN_DOCKER={}", in_docker);

        if !in_docker {
            tracing::warn!("[commands::restart] Restart rejected: not in Docker environment");
            return Ok(SubmissionResult::error(
                "Restart is not available in this environment. \
                 The IRONCLAW_IN_DOCKER environment variable must be set to 'true' for Docker deployments."
                    .to_string(),
            ));
        }

        // Execute restart tool directly (don't dispatch as a job for LLM planning)
        // This ensures the tool runs immediately without LLM involvement
        use crate::tools::Tool;
        let tool = crate::tools::builtin::RestartTool;
        let params = serde_json::json!({});

        // Create a minimal JobContext for the tool
        let dummy_ctx =
            crate::context::JobContext::with_user("system", "Restart", "Graceful restart");

        match tool.execute(params, &dummy_ctx).await {
            Ok(output) => {
                tracing::info!("[commands::restart] RestartTool executed successfully");
                // Extract text from the ToolOutput result
                let response = match output.result {
                    serde_json::Value::String(s) => s,
                    _ => output.result.to_string(),
                };
                Ok(SubmissionResult::response(response))
            }
            Err(e) => {
                tracing::error!("[commands::restart] RestartTool execution failed: {:?}", e);
                Ok(SubmissionResult::error(format!("Restart failed: {}", e)))
            }
        }
    }

    /// Handle `/skills [search <query>]`, routing to list or search.
    async fn handle_skills_command(&self, args: &[String]) -> Result<SubmissionResult, Error> {
        if args.first().map(|s| s.as_str()) == Some("search") {
            let query = args[1..].join(" ");
            if query.is_empty() {
                return Ok(SubmissionResult::error("Usage: /skills search <query>"));
            }
            self.handle_skills_search(&query).await
        } else if args.is_empty() {
            self.handle_skills_list().await
        } else {
            Ok(SubmissionResult::error(
                "Usage: /skills or /skills search <query>",
            ))
        }
    }

    /// Render the active model and the available model list for `/model`.
    async fn show_models(&self) -> String {
        let current = self.llm().active_model_name();
        let mut out = format!("Active model: {}\n", current);
        match self.llm().list_models().await {
            Ok(models) if !models.is_empty() => {
                out.push_str("\nAvailable models:\n");
                for m in &models {
                    let marker = if *m == current { " (active)" } else { "" };
                    out.push_str(&format!("  {}{}\n", m, marker));
                }
                out.push_str("\nUse /model <name> to switch.");
            }
            Ok(_) => {
                out.push_str("\nCould not fetch model list. Use /model <name> to switch.");
            }
            Err(e) => {
                out.push_str(&format!(
                    "\nCould not fetch models: {}. Use /model <name> to switch.",
                    e
                ));
            }
        }
        out
    }

    /// Validate (best-effort) and switch to the requested model, persisting
    /// the choice on success.
    async fn switch_model(&self, requested: &str) -> Result<SubmissionResult, Error> {
        // Validate the model exists
        match self.llm().list_models().await {
            Ok(models) if !models.is_empty() => {
                if !models.iter().any(|m| m == requested) {
                    return Ok(SubmissionResult::error(format!(
                        "Unknown model: {}. Available models:\n  {}",
                        requested,
                        models.join("\n  ")
                    )));
                }
            }
            Ok(_) => {
                // Empty model list, can't validate but try anyway
            }
            Err(e) => {
                tracing::warn!("Could not fetch model list for validation: {}", e);
            }
        }

        match self.llm().set_model(requested) {
            Ok(()) => {
                // Persist the model choice so it survives restarts.
                self.persist_selected_model(requested).await;
                Ok(SubmissionResult::response(format!(
                    "Switched model to: {}",
                    requested
                )))
            }
            Err(e) => Ok(SubmissionResult::error(format!(
                "Failed to switch model: {}",
                e
            ))),
        }
    }

    /// Handle legacy command routing from the Router (job commands that go through
    /// process_user_input -> router -> handle_job_or_command -> here).
    pub(super) async fn handle_command(
        &self,
        command: &str,
        args: &[String],
        channel: &str,
    ) -> Result<Option<String>, Error> {
        // System commands are now handled directly via Submission::SystemCommand,
        // but the router may still send us unknown /commands.
        match self.handle_system_command(command, args, channel).await? {
            SubmissionResult::Response { content } => Ok(Some(content)),
            SubmissionResult::Ok { message } => Ok(message),
            SubmissionResult::Error { message } => Ok(Some(format!("Error: {}", message))),
            _ => Ok(None),
        }
    }

    /// Persist the selected model to the settings store (DB and/or TOML config).
    ///
    /// Best-effort: logs warnings on failure but does not propagate errors,
    /// since the in-memory model switch already succeeded.
    async fn persist_selected_model(&self, model: &str) {
        // 1. Persist to DB if available.
        if let Some(store) = self.store() {
            let value = serde_json::Value::String(model.to_string());
            if let Err(e) = store
                .set_setting(
                    crate::db::UserId::from("default"),
                    crate::db::SettingKey::from("selected_model"),
                    &value,
                )
                .await
            {
                tracing::warn!("Failed to persist model to DB: {}", e);
            }
        }

        // 2. Update TOML config file if it exists (sync I/O in spawn_blocking).
        let model_owned = model.to_string();
        if let Err(e) = tokio::task::spawn_blocking(move || {
            let toml_path = crate::settings::Settings::default_toml_path();
            match crate::settings::Settings::load_toml(&toml_path) {
                Ok(Some(mut settings)) => {
                    settings.selected_model = Some(model_owned);
                    if let Err(e) = settings.save_toml(&toml_path) {
                        tracing::warn!("Failed to persist model to config.toml: {}", e);
                    }
                }
                Ok(None) => {
                    // No config file on disk; nothing to update.
                }
                Err(e) => {
                    tracing::warn!("Failed to load config.toml for model persistence: {}", e);
                }
            }
        })
        .await
        {
            tracing::warn!("Model TOML persistence task failed: {}", e);
        }
    }
}
