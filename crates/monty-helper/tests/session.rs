//! End-to-end tests that drive the helper binary over its framed protocol.

use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use monty_protocol::{
    CallOutcome, DenialCode, GuestLimits, HelperToParent, ParentToHelper, ResumeRun, StartRun,
    TerminalOutcome, ToolDefinition, decode_frame, read_frame, write_frame,
};
use serde_json::{Value, json};

struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Helper {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_axinite-monty-helper"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn the helper");
        let stdin = child.stdin.take().expect("helper stdin");
        let stdout = BufReader::new(child.stdout.take().expect("helper stdout"));
        let mut helper = Self {
            child,
            stdin,
            stdout,
        };
        assert!(matches!(helper.read(), HelperToParent::Hello(_)));
        helper
    }

    fn send(&mut self, message: &ParentToHelper) {
        write_frame(&mut self.stdin, message).expect("write a frame");
        self.stdin.flush().expect("flush stdin");
    }

    fn read(&mut self) -> HelperToParent {
        let payload = read_frame(&mut self.stdout).expect("read a frame");
        decode_frame(&payload).expect("decode a frame")
    }

    fn start_run(&mut self, request: &StartRun) {
        self.send(&ParentToHelper::StartRun(request.clone()));
    }

    fn terminal(&mut self) -> TerminalOutcome {
        match self.read() {
            HelperToParent::Terminal(terminal) => terminal.outcome,
            other => panic!("expected a terminal message, got {other:?}"),
        }
    }

    fn answer(&mut self, call: monty_protocol::HostCall, value: Value) {
        self.send(&ParentToHelper::ResumeRun(ResumeRun {
            run_id: call.run_id,
            call_id: call.call_id,
            outcome: CallOutcome::Value { value },
        }));
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn limits() -> GuestLimits {
    GuestLimits {
        max_memory_bytes: 64 * 1024 * 1024,
        max_recursion_depth: 256,
        max_suspensions: 4096,
        max_feed_millis: 2_000,
        max_turn_millis: 2_000,
        max_output_bytes: 65_536,
    }
}

fn request(code: &str, tools: Vec<ToolDefinition>) -> StartRun {
    StartRun {
        run_id: "run_1".to_owned(),
        code: code.to_owned(),
        entrypoint: "main".to_owned(),
        params: json!({"count": 42}),
        state: json!({}),
        tools,
        limits: limits(),
    }
}

fn echo_tool() -> ToolDefinition {
    ToolDefinition {
        name: "echo".to_owned(),
        description: "Echo the supplied text.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
            "additionalProperties": false
        }),
    }
}

#[test]
fn completes_with_result_and_state() {
    let mut helper = Helper::start();
    helper.start_run(&request(
        "return {'result': params['count'] + 1, 'state': state}",
        Vec::new(),
    ));
    match helper.terminal() {
        TerminalOutcome::Completed { result, state } => {
            assert_eq!(result, json!(43));
            assert_eq!(state, json!({}));
        }
        other => panic!("expected completion, got {other:?}"),
    }
}

#[test]
fn mediates_a_tool_call() {
    let mut helper = Helper::start();
    helper.start_run(&request(
        "value = await echo(text='hi')\nreturn {'result': value, 'state': state}",
        vec![echo_tool()],
    ));
    match helper.read() {
        HelperToParent::HostCall(call) => {
            assert_eq!(call.name, "echo");
            assert_eq!(call.arguments, json!({"text": "hi"}));
            helper.answer(call, json!({"echoed": "hi"}));
        }
        other => panic!("expected a host call, got {other:?}"),
    }
    match helper.terminal() {
        TerminalOutcome::Completed { result, .. } => {
            assert_eq!(result, json!({"echoed": "hi"}));
        }
        other => panic!("expected completion, got {other:?}"),
    }
}

#[test]
fn reports_a_tool_termination() {
    let mut helper = Helper::start();
    helper.start_run(&request(
        "value = await echo(text='hi')\nreturn {'result': value, 'state': state}",
        vec![echo_tool()],
    ));
    let HelperToParent::HostCall(call) = helper.read() else {
        panic!("expected a host call");
    };
    helper.send(&ParentToHelper::ResumeRun(ResumeRun {
        run_id: call.run_id,
        call_id: call.call_id,
        outcome: CallOutcome::Terminate {
            code: DenialCode::ToolFailed,
            message: "upstream failed".to_owned(),
        },
    }));
    match helper.terminal() {
        TerminalOutcome::ToolError { code, message } => {
            assert_eq!(code, DenialCode::ToolFailed);
            assert_eq!(message, "upstream failed");
        }
        other => panic!("expected a tool error, got {other:?}"),
    }
}

#[test]
fn reports_a_script_error() {
    let mut helper = Helper::start();
    helper.start_run(&request("raise ValueError('boom')", Vec::new()));
    match helper.terminal() {
        TerminalOutcome::ScriptError { message } => {
            assert!(message.contains("ValueError"), "{message}");
            assert!(message.contains("boom"), "{message}");
        }
        other => panic!("expected a script error, got {other:?}"),
    }
}

#[test]
fn requires_the_returned_state_object() {
    let mut helper = Helper::start();
    helper.start_run(&request("return {'result': 1}", Vec::new()));
    match helper.terminal() {
        TerminalOutcome::ScriptError { message } => {
            assert!(message.contains("state"), "{message}");
        }
        other => panic!("expected a script error, got {other:?}"),
    }
}

#[test]
fn denies_operating_system_access() {
    let mut helper = Helper::start();
    helper.start_run(&request(
        "open('/etc/passwd')\nreturn {'result': None, 'state': state}",
        Vec::new(),
    ));
    match helper.terminal() {
        TerminalOutcome::ScriptError { message } => {
            assert!(message.contains("operating-system"), "{message}");
        }
        other => panic!("expected a script error, got {other:?}"),
    }
}

#[test]
fn denies_concurrent_tool_calls() {
    let mut helper = Helper::start();
    helper.start_run(&request(
        "import asyncio\nfirst, second = await asyncio.gather(echo(text='a'), echo(text='b'))\nreturn {'result': [first, second], 'state': state}",
        vec![echo_tool()],
    ));
    let HelperToParent::HostCall(call) = helper.read() else {
        panic!("expected a host call");
    };
    helper.answer(call, json!("a"));
    match helper.terminal() {
        TerminalOutcome::ScriptError { message } => {
            assert!(message.contains("concurrent tool calls"), "{message}");
        }
        other => panic!("expected a script error, got {other:?}"),
    }
}

#[test]
fn enforces_the_feed_time_limit() {
    let mut helper = Helper::start();
    let mut request = request("while True:\n    pass", Vec::new());
    request.limits.max_feed_millis = 100;
    request.limits.max_turn_millis = 100;
    helper.start_run(&request);
    match helper.terminal() {
        TerminalOutcome::ResourceLimitExceeded { message } => {
            assert!(message.contains("time limit"), "{message}");
        }
        other => panic!("expected a resource limit, got {other:?}"),
    }
}

#[test]
fn rejects_a_reserved_entrypoint() {
    let mut helper = Helper::start();
    let mut request = request("return {'state': state}", Vec::new());
    request.entrypoint = "state".to_owned();
    helper.start_run(&request);
    match helper.terminal() {
        TerminalOutcome::RuntimeCrash { message } => {
            assert!(message.contains("reserved"), "{message}");
        }
        other => panic!("expected a rejected request, got {other:?}"),
    }
}
