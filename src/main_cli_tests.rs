//! Unit tests for CLI dispatch routing in [`super`].
//! Exercises the dispatcher functions that route CLI commands to either
//! synchronous tool handlers, asynchronous local handlers, or
//! agent-specific executors, verifying passthrough and short-circuit
//! behaviour.

use std::sync::Mutex;

use ironclaw::cli::{Cli, Command, PairingCommand};
use rstest::rstest;

use super::{
    dispatch_agent_commands, dispatch_cli_tool_commands, dispatch_subcommand, is_agent_subcommand,
    test_support,
};

/// Constructs a `Cli` instance with the given command and default flags.
fn cli_with(command: Option<Command>) -> Cli {
    Cli {
        command,
        cli_only: false,
        no_db: false,
        message: None,
        config: None,
        no_onboard: false,
    }
}

/// Asserts that `dispatch_cli_tool_commands` returns `Ok(None)` for the
/// given passthrough command variant.
async fn assert_tool_commands_passthrough(command: Command) {
    let cli = cli_with(Some(command));
    let result = dispatch_cli_tool_commands(&cli)
        .await
        .expect("dispatch should succeed");
    assert!(result.is_none());
}

/// RAII guard installing an agent dispatch hook for the duration of the test.
struct AgentDispatchHookGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl AgentDispatchHookGuard {
    /// Installs the hook and returns a guard that clears it on drop.
    fn install(hook: test_support::AgentDispatchHook) -> Self {
        let guard = test_support::AGENT_DISPATCH_LOCK
            .lock()
            .expect("agent dispatch lock should not be poisoned");
        *test_support::AGENT_DISPATCH_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("agent dispatch hook mutex should not be poisoned") = Some(hook);
        Self { _guard: guard }
    }
}

impl Drop for AgentDispatchHookGuard {
    fn drop(&mut self) {
        *test_support::AGENT_DISPATCH_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("agent dispatch hook mutex should not be poisoned") = None;
    }
}

/// Test hook that immediately returns `Ok(Some(true)).`
fn handled_agent_command(_: &Command) -> anyhow::Result<Option<bool>> {
    Ok(Some(true))
}

/// Constructs a `Command::Worker` with placeholder values.
fn worker_command() -> Command {
    Command::Worker {
        job_id: uuid::Uuid::new_v4(),
        orchestrator_url: "http://localhost".into(),
        max_iterations: 10,
    }
}

/// Constructs a `Command::ClaudeBridge` with placeholder values.
fn claude_bridge_command() -> Command {
    Command::ClaudeBridge {
        job_id: uuid::Uuid::new_v4(),
        orchestrator_url: "http://localhost".into(),
        max_turns: 5,
        model: "claude-3".into(),
    }
}

/// Constructs a `Command::Pairing(PairingCommand::List)`.
fn pairing_list_command() -> Command {
    Command::Pairing(PairingCommand::List {
        channel: "telegram".to_string(),
        json: false,
    })
}

/// Constructs a `Command::Onboard` with default flags.
fn onboard_command() -> Command {
    Command::Onboard {
        skip_auth: false,
        channels_only: false,
        provider_only: false,
        quick: false,
    }
}

/// Constructs a `Command::Status`.
fn status_command() -> Command {
    Command::Status
}

/// Asserts that `dispatch_subcommand` returns `true` for the given command.
async fn assert_subcommand_short_circuits(command: Command) {
    let cli = cli_with(Some(command));
    let dispatched = dispatch_subcommand(&cli)
        .await
        .expect("dispatch should succeed");
    assert!(dispatched);
}

#[tokio::test]
async fn tool_commands_returns_none_for_no_command() {
    let cli = cli_with(None);
    let result = dispatch_cli_tool_commands(&cli)
        .await
        .expect("dispatch should succeed");
    assert!(result.is_none());
}

#[tokio::test]
async fn tool_commands_returns_none_for_run() {
    assert_tool_commands_passthrough(Command::Run).await;
}

#[rstest]
#[case(worker_command())]
#[case(claude_bridge_command())]
#[case(onboard_command())]
#[tokio::test]
async fn tool_commands_returns_none_for_agent_passthrough_variants(#[case] command: Command) {
    assert_tool_commands_passthrough(command).await;
}

#[tokio::test]
async fn tool_commands_returns_some_for_pairing_list() {
    let cli = cli_with(Some(pairing_list_command()));
    let result = dispatch_cli_tool_commands(&cli)
        .await
        .expect("dispatch should succeed");

    assert_eq!(result, Some(true));
}

#[tokio::test]
async fn tool_commands_returns_some_for_status_async() {
    let cli = cli_with(Some(status_command()));
    let result = dispatch_cli_tool_commands(&cli)
        .await
        .expect("dispatch should succeed");

    assert_eq!(result, Some(true));
}

#[tokio::test]
async fn agent_commands_returns_none_for_run() {
    let cli = cli_with(Some(Command::Run));
    let result = dispatch_agent_commands(&cli)
        .await
        .expect("dispatch should succeed");
    assert!(result.is_none());
}

#[tokio::test]
async fn agent_commands_returns_none_for_no_command() {
    let cli = cli_with(None);
    let result = dispatch_agent_commands(&cli)
        .await
        .expect("dispatch should succeed");
    assert!(result.is_none());
}

#[test]
fn is_agent_subcommand_identifies_agent_only_variants() {
    assert!(is_agent_subcommand(&Command::Run));
    assert!(is_agent_subcommand(&onboard_command()));
    assert!(!is_agent_subcommand(&pairing_list_command()));
}

#[tokio::test]
async fn agent_commands_returns_some_for_handled_worker_command() {
    let _hook = AgentDispatchHookGuard::install(handled_agent_command);
    let cli = cli_with(Some(worker_command()));

    let result = dispatch_agent_commands(&cli)
        .await
        .expect("dispatch should succeed");

    assert_eq!(result, Some(true));
}

#[tokio::test]
async fn dispatch_subcommand_short_circuits_for_pairing_command() {
    assert_subcommand_short_circuits(pairing_list_command()).await;
}

#[tokio::test]
async fn dispatch_subcommand_short_circuits_for_handled_worker_command() {
    let _hook = AgentDispatchHookGuard::install(handled_agent_command);
    assert_subcommand_short_circuits(worker_command()).await;
}
