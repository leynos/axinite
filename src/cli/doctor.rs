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
    check_database_with_context, check_llm_config_with_context, check_nearai_session,
    check_settings_file, check_workspace_dir, check_workspace_search_with_context,
};
use external_checks::{check_binary, check_docker_daemon, check_service_installed};
use subsystem_checks::{
    check_embeddings, check_gateway_config, check_mcp_config, check_routines_config, check_secrets,
    check_skills,
};

use crate::config::EnvContext;
use crate::settings::Settings;

async fn run_core_checks(
    env_context: &EnvContext,
    settings: &Settings,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
) {
    check(
        "Settings file",
        check_settings_file(env_context),
        passed,
        failed,
        skipped,
    );
    check(
        "NEAR AI session",
        check_nearai_session(env_context).await,
        passed,
        failed,
        skipped,
    );
    check(
        "LLM configuration",
        check_llm_config_with_context(env_context, settings),
        passed,
        failed,
        skipped,
    );
    check(
        "Database backend",
        check_database_with_context(env_context).await,
        passed,
        failed,
        skipped,
    );
    check(
        "Workspace search",
        check_workspace_search_with_context(env_context).await,
        passed,
        failed,
        skipped,
    );
    check(
        "Workspace directory",
        check_workspace_dir(env_context),
        passed,
        failed,
        skipped,
    );
}

async fn run_subsystem_checks(
    settings: &Settings,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
) {
    check(
        "Embeddings",
        check_embeddings(settings),
        passed,
        failed,
        skipped,
    );
    check(
        "Routines config",
        check_routines_config(),
        passed,
        failed,
        skipped,
    );
    check(
        "Gateway config",
        check_gateway_config(settings),
        passed,
        failed,
        skipped,
    );
    check(
        "MCP servers",
        check_mcp_config().await,
        passed,
        failed,
        skipped,
    );
    check("Skills", check_skills().await, passed, failed, skipped);
    check("Secrets", check_secrets(settings), passed, failed, skipped);
    check(
        "Service",
        check_service_installed(),
        passed,
        failed,
        skipped,
    );
}

async fn run_external_binary_checks(passed: &mut u32, failed: &mut u32, skipped: &mut u32) {
    check(
        "Docker daemon",
        check_docker_daemon().await,
        passed,
        failed,
        skipped,
    );
    check(
        "cloudflared",
        check_binary("cloudflared", &["--version"]),
        passed,
        failed,
        skipped,
    );
    check(
        "ngrok",
        check_binary("ngrok", &["version"]),
        passed,
        failed,
        skipped,
    );
    check(
        "tailscale",
        check_binary("tailscale", &["version"]),
        passed,
        failed,
        skipped,
    );
}

/// Run all diagnostic checks and print results.
pub async fn run_doctor_command() -> anyhow::Result<()> {
    println!("Axinite Doctor");
    println!("===============\n");

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;

    // Capture the environment once and derive settings from the same snapshot
    // so every check observes a consistent view.
    let env_context = crate::bootstrap::capture_config_env_context();
    let settings = Settings::load_from(&env_context.axinite_base_dir().join("settings.json"));

    run_core_checks(
        &env_context,
        &settings,
        &mut passed,
        &mut failed,
        &mut skipped,
    )
    .await;
    run_subsystem_checks(&settings, &mut passed, &mut failed, &mut skipped).await;
    run_external_binary_checks(&mut passed, &mut failed, &mut skipped).await;

    // ── Summary ───────────────────────────────────────────────

    println!();
    println!("  {passed} passed, {failed} failed, {skipped} skipped");

    if failed > 0 {
        println!("\n  Some checks failed. This is normal if you don't use those features.");
    }

    Ok(())
}

// ── Check runner ────────────────────────────────────────────

fn check(name: &str, result: CheckResult, passed: &mut u32, failed: &mut u32, skipped: &mut u32) {
    match result {
        CheckResult::Pass(detail) => {
            *passed += 1;
            println!("  [pass] {name}: {detail}");
        }
        CheckResult::Fail(detail) => {
            *failed += 1;
            println!("  [FAIL] {name}: {detail}");
        }
        CheckResult::Skip(reason) => {
            *skipped += 1;
            println!("  [skip] {name}: {reason}");
        }
    }
}

enum CheckResult {
    Pass(String),
    Fail(String),
    Skip(String),
}
