//! `axinite doctor` - active health diagnostics.
//!
//! Probes external dependencies and validates configuration to surface
//! problems before they bite during normal operation. Each check reports
//! pass/fail with actionable guidance on failures.

mod core_checks;
mod external_checks;
mod subsystem_checks;
#[cfg(test)]
mod tests;

use core_checks::{
    check_database, check_nearai_session, check_settings_file, check_workspace_dir,
    check_workspace_search,
};
use external_checks::{check_binary, check_docker_daemon, check_service_installed};
use subsystem_checks::{
    check_embeddings_with_context, check_gateway_config_with_context, check_mcp_config,
    check_routines_config_with_context, check_secrets, check_skills,
};

use crate::config::EnvContext;
use crate::settings::Settings;

async fn run_core_checks(inputs: &DoctorCheckInputs<'_>, results: &mut CheckCounts) {
    check("Settings file", check_settings_file(), results);
    check(
        "NEAR AI session",
        check_nearai_session(inputs.env_context).await,
        results,
    );
    check(
        "LLM configuration",
        core_checks::check_llm_config_with_context(inputs.env_context, inputs.settings),
        results,
    );
    check(
        "Database backend",
        check_database(inputs.env_context).await,
        results,
    );
    check(
        "Workspace search",
        check_workspace_search(inputs.env_context).await,
        results,
    );
    check(
        "Workspace directory",
        check_workspace_dir(inputs.env_context),
        results,
    );
}

async fn run_subsystem_checks(inputs: &DoctorCheckInputs<'_>, results: &mut CheckCounts) {
    check(
        "Embeddings",
        check_embeddings_with_context(inputs.env_context, inputs.settings),
        results,
    );
    check(
        "Routines config",
        check_routines_config_with_context(inputs.env_context),
        results,
    );
    check(
        "Gateway config",
        check_gateway_config_with_context(inputs.env_context, inputs.settings),
        results,
    );
    check("MCP servers", check_mcp_config().await, results);
    check("Skills", check_skills(inputs.env_context).await, results);
    check(
        "Secrets",
        check_secrets(inputs.settings, inputs.env_context),
        results,
    );
    check("Service", check_service_installed(), results);
}

async fn run_external_binary_checks(results: &mut CheckCounts) {
    check("Docker daemon", check_docker_daemon().await, results);
    check(
        "cloudflared",
        check_binary("cloudflared", &["--version"]),
        results,
    );
    check("ngrok", check_binary("ngrok", &["version"]), results);
    check(
        "tailscale",
        check_binary("tailscale", &["version"]),
        results,
    );
}

/// Run all diagnostic checks and print results.
pub async fn run_doctor_command() -> anyhow::Result<()> {
    println!("Axinite Doctor");
    println!("===============\n");

    let mut results = CheckCounts::default();

    // Load settings once for checks that need them.
    EnvContext::refresh_dotenv_layers();
    let env_context = EnvContext::capture_ambient();
    let settings = Settings::load();
    let inputs = DoctorCheckInputs {
        settings: &settings,
        env_context: &env_context,
    };

    run_core_checks(&inputs, &mut results).await;
    run_subsystem_checks(&inputs, &mut results).await;
    run_external_binary_checks(&mut results).await;

    // ── Summary ───────────────────────────────────────────────

    println!();
    println!(
        "  {} passed, {} failed, {} skipped",
        results.passed, results.failed, results.skipped
    );

    if results.failed > 0 {
        println!("\n  Some checks failed. This is normal if you don't use those features.");
    }

    Ok(())
}

// ── Check runner ────────────────────────────────────────────

struct DoctorCheckInputs<'a> {
    settings: &'a Settings,
    env_context: &'a EnvContext,
}

#[derive(Default)]
struct CheckCounts {
    passed: u32,
    failed: u32,
    skipped: u32,
}

fn check(name: &str, result: CheckResult, counts: &mut CheckCounts) {
    match result {
        CheckResult::Pass(detail) => {
            counts.passed += 1;
            println!("  [pass] {name}: {detail}");
        }
        CheckResult::Fail(detail) => {
            counts.failed += 1;
            println!("  [FAIL] {name}: {detail}");
        }
        CheckResult::Skip(reason) => {
            counts.skipped += 1;
            println!("  [skip] {name}: {reason}");
        }
    }
}

enum CheckResult {
    Pass(String),
    Fail(String),
    Skip(String),
}
