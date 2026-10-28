//! Public API behaviour for parsing stored and requested sandbox modes.

use axinite::db::SandboxMode;
use rstest::rstest;

#[rstest]
#[case::worker("worker", SandboxMode::Worker)]
#[case::claude_code("claude_code", SandboxMode::ClaudeCode)]
fn parses_supported_sandbox_modes(#[case] input: &str, #[case] expected: SandboxMode) {
    assert_eq!(input.parse::<SandboxMode>(), Ok(expected));
}

#[test]
fn reports_the_unexpected_sandbox_mode() {
    let error = "interactive"
        .parse::<SandboxMode>()
        .expect_err("unknown sandbox modes should fail to parse");

    assert_eq!(error.to_string(), "unexpected sandbox mode 'interactive'");
}
