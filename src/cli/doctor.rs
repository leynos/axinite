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
    check_embeddings_with_context, check_gateway_config_with_context,
    check_mcp_config_with_context, check_routines_config_with_context, check_secrets_with_context,
    check_skills_with_context,
};

use super::report::{Report, outln};
use crate::config::EnvContext;
use crate::settings::Settings;

async fn run_core_checks<W: std::io::Write>(
    env_context: &EnvContext,
    settings: &Settings,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report<W>,
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

async fn run_subsystem_checks<W: std::io::Write>(
    env_context: &EnvContext,
    settings: &Settings,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report<W>,
) {
    check(
        "Embeddings",
        check_embeddings_with_context(env_context, settings),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Routines config",
        check_routines_config_with_context(env_context),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Gateway config",
        check_gateway_config_with_context(env_context, settings),
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "MCP servers",
        check_mcp_config_with_context(env_context).await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Skills",
        check_skills_with_context(env_context).await,
        passed,
        failed,
        skipped,
        report,
    );
    check(
        "Secrets",
        check_secrets_with_context(env_context, settings),
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

async fn run_external_binary_checks<W: std::io::Write>(
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report<W>,
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
#[cfg(test)]
pub(crate) async fn doctor_report(env_context: &EnvContext) -> String {
    let mut report = Report::new(Vec::new());
    render_doctor_report(env_context, &mut report).await;
    report.into_string()
}

/// Stream the diagnostic report to `report` as each check completes.
async fn render_doctor_report<W: std::io::Write>(env_context: &EnvContext, report: &mut Report<W>) {
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
        report,
    )
    .await;
    run_subsystem_checks(
        env_context,
        &settings,
        &mut passed,
        &mut failed,
        &mut skipped,
        report,
    )
    .await;
    run_external_binary_checks(&mut passed, &mut failed, &mut skipped, report).await;

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
}

/// Run all diagnostic checks and print results.
pub async fn run_doctor_command() -> anyhow::Result<()> {
    let env_context = crate::bootstrap::capture_config_env_context();
    let mut report = Report::new(std::io::stdout().lock());
    render_doctor_report(&env_context, &mut report).await;
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

fn check<W: std::io::Write>(
    name: &str,
    result: CheckResult,
    passed: &mut u32,
    failed: &mut u32,
    skipped: &mut u32,
    report: &mut Report<W>,
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
