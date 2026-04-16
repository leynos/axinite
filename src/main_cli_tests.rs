use std::sync::Mutex;

use ironclaw::cli::{Cli, Command, PairingCommand};
use rstest::rstest;

use super::{
    dispatch_agent_commands, dispatch_cli_tool_commands, dispatch_subcommand, is_agent_subcommand,
    test_support,
};

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

struct AgentDispatchHookGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl AgentDispatchHookGuard {
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

fn handled_agent_command(_: &Command) -> anyhow::Result<Option<bool>> {
    Ok(Some(true))
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
#[case(Command::Worker {
    job_id: uuid::Uuid::new_v4(),
    orchestrator_url: "http://localhost".into(),
    max_iterations: 10,
})]
#[case(Command::ClaudeBridge {
    job_id: uuid::Uuid::new_v4(),
    orchestrator_url: "http://localhost".into(),
    max_turns: 5,
    model: "claude-3".into(),
})]
#[case(Command::Onboard {
    skip_auth: false,
    channels_only: false,
    provider_only: false,
    quick: false,
})]
#[tokio::test]
async fn tool_commands_returns_none_for_agent_passthrough_variants(#[case] command: Command) {
    let cli = cli_with(Some(command));
    let result = dispatch_cli_tool_commands(&cli)
        .await
        .expect("dispatch should succeed");
    assert!(result.is_none());
}

#[tokio::test]
async fn tool_commands_returns_some_for_pairing_list() {
    let cli = cli_with(Some(Command::Pairing(PairingCommand::List {
        channel: "telegram".to_string(),
        json: false,
    })));
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
    assert!(is_agent_subcommand(&Command::Onboard {
        skip_auth: false,
        channels_only: false,
        provider_only: false,
        quick: false,
    }));
    assert!(!is_agent_subcommand(&Command::Pairing(
        PairingCommand::List {
            channel: "telegram".to_string(),
            json: false,
        }
    )));
}

#[tokio::test]
async fn agent_commands_returns_some_for_handled_worker_command() {
    let _hook = AgentDispatchHookGuard::install(handled_agent_command);
    let cli = cli_with(Some(Command::Worker {
        job_id: uuid::Uuid::new_v4(),
        orchestrator_url: "http://localhost".into(),
        max_iterations: 10,
    }));

    let result = dispatch_agent_commands(&cli)
        .await
        .expect("dispatch should succeed");

    assert_eq!(result, Some(true));
}

#[tokio::test]
async fn dispatch_subcommand_short_circuits_for_pairing_command() {
    let cli = cli_with(Some(Command::Pairing(PairingCommand::List {
        channel: "telegram".to_string(),
        json: false,
    })));

    let dispatched = dispatch_subcommand(&cli)
        .await
        .expect("dispatch should succeed");

    assert!(dispatched);
}

#[tokio::test]
async fn dispatch_subcommand_short_circuits_for_handled_worker_command() {
    let _hook = AgentDispatchHookGuard::install(handled_agent_command);
    let cli = cli_with(Some(Command::Worker {
        job_id: uuid::Uuid::new_v4(),
        orchestrator_url: "http://localhost".into(),
        max_iterations: 10,
    }));

    let dispatched = dispatch_subcommand(&cli)
        .await
        .expect("dispatch should succeed");

    assert!(dispatched);
}
