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

use super::report::{Report, outln};
use crate::config::EnvContext;
use crate::settings::Settings;

async fn run_core_checks(
    env_context: &EnvContext,
    settings: &Settings,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report,
) {
    check(
        "Settings file",
        check_settings_file(env_context),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "NEAR AI session",
        check_nearai_session(env_context).await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "LLM configuration",
        check_llm_config_with_context(env_context, settings),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Database backend",
        check_database_with_context(env_context).await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Workspace search",
        check_workspace_search_with_context(env_context).await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Workspace directory",
        check_workspace_dir(env_context),
        passed,
        failed,
        skipped,
        report,
    );
}

async fn run_subsystem_checks(
    settings: &Settings,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report,
) {
    check(
        "Embeddings",
        check_embeddings(settings),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Routines config",
        check_routines_config(),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Gateway config",
        check_gateway_config(settings),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "MCP servers",
        check_mcp_config().await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Skills",
        check_skills().await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Secrets",
        check_secrets(settings),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Service",
        check_service_installed(),
        passed,
        failed,
        skipped,
        report,
    );
}

async fn run_external_binary_checks(
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report,
) {
    check(
        "Docker daemon",
        check_docker_daemon().await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "cloudflared",
        check_binary("cloudflared", &["--version"]),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "ngrok",
        check_binary("ngrok", &["version"]),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "tailscale",
        check_binary("tailscale", &["version"]),
        passed,
        failed,
        skipped,
        report,
    );
}

/// Render the diagnostic report for the supplied environment snapshot.
///
/// Every check and the settings it uses come from `env_context`, so a caller
/// supplying a divergent `AXINITE_BASE_DIR` inspects that deployment. The
/// returned text is exactly what [`run_doctor_command`] prints.
pub(crate) async fn doctor_report(env_context: &EnvContext) -> String {
    let mut report = Report::default();
    outln!(report, "Axinite Doctor");
    outln!(report, "===============\n");

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;

    let settings = settings_for_context(env_context);

    run_core_checks(
        env_context,
        &settings,
        &mut passed,
        &mut failed,
        &mut skipped,
        &mut report,
    )
    .await;
    run_subsystem_checks(
        &settings,
        &mut passed,
        &mut failed,
        &mut skipped,
        &mut report,
    )
    .await;
    run_external_binary_checks(&mut passed, &mut failed, &mut skipped, &mut report).await;

    // ── Summary ───────────────────────────────────────────────

    outln!(report, "");
    outln!(
        report,
        "  {passed} passed, {failed} failed, {skipped} skipped"
    );

    if failed > 0 {
        outln!(
            report,
            "\n  Some checks failed. This is normal if you don't use those features."
        );
    }

    report.into_string()
}

/// Run all diagnostic checks and print results.
pub async fn run_doctor_command() -> anyhow::Result<()> {
    let env_context = crate::bootstrap::capture_config_env_context();
    print!("{}", doctor_report(&env_context).await);
    Ok(())
}

/// Load doctor's settings from the supplied snapshot's base directory.
///
/// A malformed settings file must not abort diagnostics: the warning is
/// printed and the default settings are used instead.
pub(crate) fn settings_for_context(env_context: &EnvContext) -> Settings {
    match Settings::load_from(&env_context.axinite_base_dir().join("settings.json")) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("Warning: {error}; continuing with default settings");
            Settings::default()
        }
    }
}

// ── Check runner ────────────────────────────────────────────

fn check(
    name: &str,
    result: CheckResult,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report,
) {
    match result {
        CheckResult::Pass(detail) => {
            *passed += 1;
            outln!(report, "  [pass] {name}: {detail}");
        }
        CheckResult::Fail(detail) => {
            *failed += 1;
            outln!(report, "  [FAIL] {name}: {detail}");
        }
        CheckResult::Skip(reason) => {
            *skipped += 1;
            outln!(report, "  [skip] {name}: {reason}");
        }
    }
}

enum CheckResult {
    Pass(String),
    Fail(String),
    Skip(String),
}
