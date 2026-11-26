# Architectural decision record (ADR) 0016: Isolated Monty execution

## Status

Accepted (2026-11-18). `exec_code` runs agent-authored Python in a separate
helper process linked against Monty 1.6.0. The parent never links the
interpreter, owns every side effect, and mediates each guest callback through
the caller's ordinary policy path. Only the chat caller adapter issues the
authority a run needs in 2.2.1; every other caller fails closed.

## Date

2026-11-18

## Context and problem statement

Axinite gives agents a catalogue of policy-gated tools, but no way to compose
them in a program. RFC 0005 proposes a Monty-based codemode runner so an agent
can fetch data with one approved tool, transform it, and call another without
round-tripping every intermediate value through the model. The first
deliverable is `exec_code`: ephemeral, JSON-only Python with an explicit
per-run tool allowlist.

Two properties dominate the design. A Monty panic must not terminate the agent
runtime, and a guest callback must never be able to widen its own authority.
Monty is a Rust library, so the first property forces a process boundary or an
unwind boundary the compiler cannot enforce. Monty supports host-controlled
external functions and pause/resume, so the second property depends entirely on
what the host does at each suspension point.

The caller context is weaker than it looks. `Tool::execute` receives a
`JobContext` that carries no session or skills policy, and the tool list
advertised in `reason_ctx.available_tools` is not itself enforced by the
dispatcher's preflight lookup. Interactive chat approval is represented by a
dispatcher continuation, not by an awaitable inside `Tool::execute`. A design
that inferred authority from the job would therefore grant more than the caller
reviewed.

## Decision drivers

- A panic in a linked interpreter is process-fatal; a Rust `catch_unwind`
  boundary does not cover allocation failure, `abort`, or stack exhaustion.
- Guest tool calls are host effects. They must pass the same identity,
  approval, hook, schema, and output-sanitization checks as any other tool
  call, and a refusal must perform no side effect.
- Capability attenuation must be exact and mechanically checked, not argued
  from a sampled test.
- The workspace MSRV lane builds on Rust 1.95; Monty 1.6.0 requires 1.98. A
  workspace member cannot satisfy both.
- A dropped async future must not orphan a child process or leak its
  concurrency permit.
- The wire boundary must be bounded before allocation, so a malicious or
  corrupted helper cannot exhaust the parent's memory.

## Requirements

### Functional requirements

- `exec_code(code, allowed_tools, params=None, state=None, entrypoint="main", limits=None)`
  runs a bounded program and returns `{"result": …, "state": {…}}`.
- `allowed_tools` is mandatory; an empty list means no callbacks are possible.
- A guest may call only selected tools, only with schema-valid arguments, and
  only when the caller's own policy would already permit the call.
- Interactive approval cannot be satisfied from inside a run; such a callback
  fails closed before dispatch.
- Recursive `exec_code` is refused.
- Cancellation, deadline expiry, malformed frames, and helper crashes produce
  typed errors and leave no live helper.

### Technical requirements

- The parent links no Monty code.
- Every frame is length-framed JSON with a hard ceiling checked before
  allocation.
- The selected-tool projection is proved against the shipped implementation.
- Child termination and reaping precede release of the run's admission permit.

## Options considered

### Option 1: run Monty in-process behind `catch_unwind`

The smallest change, and the one the caller seam makes tempting. It fails the
first requirement: a panic unwinds, but a guest that exhausts memory, aborts,
or overflows the native stack still takes the process down. `unsafe` memory
limits are also forbidden in the root workspace, so the parent could not even
bound the guest's address space. Rejected.

### Option 2: a shared long-lived interpreter service

One supervisor process owning many Monty runs. It amortizes interpreter startup
and could pool memory limits, but it reintroduces shared state between runs: a
panic in one run kills every concurrent guest, and a per-run memory budget
cannot be enforced with `setrlimit` once the process serves more than one
guest. Rejected for 2.2.1.

### Option 3: one helper process per run over framed JSON

A dedicated helper executable links Monty, applies its own resource ceilings,
and exchanges bounded JSON frames with the parent over inherited pipes. The
parent remains the only component with tool authority. A helper crash is one
failed run, and each run gets its own address space. Chosen.

### Option 4: retain the generated-tool WASM path

`tools-src/*` already provides sandboxed WebAssembly tools, and a WASM guest
could call host functions. It would inherit a mature sandbox, but Monty is a
Rust interpreter and the RFC's product surface is Python source, not a
component ABI. Reimplementing Python on the component model is a different
project. Rejected.

## Decision outcome

### Process model

`crates/monty-helper` is an out-of-workspace package named
`axinite-monty-helper`. It has an empty `[workspace]` table, declares
`rust-version = "1.98"`, and is built and tested explicitly by
`make test-monty-helper`. Excluding it keeps the workspace MSRV lane on 1.95
without weakening the Monty pin. Its `.cargo/config.toml` pins
`-Zmin-recursion-limit=1024`, because the repository-standard `-Zthreads=8`
lowers rustc's query-depth limit beneath what Monty's generated `strum` code
needs.

The helper serves exactly one run. At startup it applies a 512 MiB `RLIMIT_AS`
ceiling to itself with safe `rustix` calls, refuses a guest heap budget above
its own ceiling, and arms the `monty-alloc` allocator with the negotiated hard
limit. The parent consequently needs no `unsafe` code and no pre-exec hook.

`Supervisor::run` admits at most four helper processes across the whole process
through a semaphore and fails fast rather than queueing. It resolves the
executable from `AXINITE_MONTY_HELPER`, then beside the parent executable, so
an installed pair works without configuration. The run deadline is enforced
inside the admitted task: kill, reap, then release the permit. Dropping the
outer future relies on `kill_on_drop(true)`; a test observes `/proc/<pid>`
disappearing after the future is dropped.

### Wire protocol

`crates/monty-protocol` is the only shared crate. It never links the
interpreter and performs no I/O beyond caller-supplied readers and writers.
Frames are a four-byte big-endian length followed by exactly that many bytes of
UTF-8 JSON; zero-length, oversized, truncated, and trailing data are rejected,
and the 256 KiB ceiling is checked before the payload buffer is allocated. JSON
values are depth- and size-bounded, and tool schemas are validated on both
sides.

The message set is `Hello`, `StartRun`, `HostCall`, `ResumeRun`, `CancelRun`,
`GuestOutput`, and `Terminal`. `PROTOCOL_VERSION` is exchanged in `hello` and
in every `start_run`; a mismatch closes the channel rather than attempting
compatibility. `Terminal` carries a tagged outcome (`completed`, `script_error`,
`tool_error`, `runtime_crash`, `timeout`, `cancelled`) so the parent does not
infer a fault class from an exit signal.

The helper validates that a callback is for the current run and the expected
call id, and the parent independently rejects a callback for a foreign run, a
replayed call id, or a terminal for another run. Stderr is drained into a
bounded buffer so a helper that writes diagnostics cannot block on a full pipe
or leak an unbounded secret sentinel into the parent's logs.

### Capability projection

`crates/monty-capabilities` holds the security-critical kernel: the selected
view is `available & requested & validated` over a bitmask of at most 64 table
entries. The shipped bodies are marked with `KERNEL-BODY` comments and repeated
verbatim in `verus/monty_capabilities.rs`, where Verus proves exactness,
non-widening in each input, admission consistency, and out-of-table rejection.
`crates/monty-capabilities/tests/drift.rs` compares the marked bodies byte for
byte, so the proved body is the compiled body. This is a deterministic
refinement bridge, not a proof-only mirror.

`BrokerContext` resolves and pins the selected tools before the helper starts.
At each callback it re-checks table membership, `Arc::ptr_eq` identity against
the current registration (a tool replaced after selection is refused), the
caller's approval snapshot, and the argument schema.
`ApprovalRequirement:: Always` and an unapproved `UnlessAutoApproved` are
denied before dispatch; `Never` and an auto-approved tool proceed. A hook may
rewrite the arguments, and every check is repeated against the rewritten call.
Hooks see redacted sensitive parameters, and results are sanitized and bounded
before they reach the guest. A denial becomes a typed `tool_error` outcome; the
guest never sees a raw host error string.

### Caller authority

`JobContext` gains an optional `Arc<BrokerContext>`. `exec_code` requires it
and returns `broker_unavailable` when absent, so scheduler, container, and
worker paths fail closed until an adapter for them is designed. The chat
adapter constructs the broker from its current registry, safety layer, hook
registry, and auto-approval snapshot. `exec_code` additionally verifies that
the requested `allowed_tools` match the broker's table exactly, so a caller
cannot request a name the broker was not built to attenuate.

### Approval

Chat approval is a dispatcher continuation (`LoopOutcome::NeedApproval`) that
cannot be awaited inside `Tool::execute`, and the worker's remote proxy reports
a `Never` requirement while the orchestrator rechecks and may return 403. There
is no safe way to satisfy an interactive prompt from inside a guest run, and
outer approval does not authorize nested effects. `exec_code` therefore rejects
any callback that would need interactive approval, and only permits effects
already eligible under the caller's existing non-interactive policy. A
continuation manager is deferred until it has its own design review.

## Known risks and limitations

- The helper is excluded from the root workspace, so `cargo check --workspace`
  does not cover it. Its own targets must run in every gate that claims to
  check the repository; `make test-monty-helper`, helper `cargo fmt` and
  `cargo clippy` are part of the commit gates.
- Binary archives produced by `cargo-dist` build the root package only. The
  helper is installed by `make install`, staged by `make test-monty-release`,
  and copied into the container images; a release archive built by `cargo-dist`
  alone does not yet contain the pair. This is recorded as a delivery gap
  rather than claimed as shipped.
- Monty 1.6.0 exposes no allocation counter, so the provisional one-million
  allocation limit is not implemented. Recursion depth, suspension count,
  feed/turn durations, output size, allocator bytes, and address space are
  enforced instead.
- Four concurrent helper processes are a process-wide admission cap, not a
  per-tenant quota. A caller that submits many runs gets fail-fast overload
  rather than fairness.
- The parent's protocol checks defend against a corrupted or malicious helper,
  not against a compromised host account that can replace the executable.
  Executable resolution trusts `AXINITE_MONTY_HELPER` and the parent's
  directory, as it must.
- The Verus proof covers the numeric projection only. Authority resolution,
  pinned identity, approval, hooks, redaction, framing, and process lifecycle
  are covered by Rust tests and real-process integration tests, not proofs.

## Consequences

- A Monty panic, abort, or memory blow-up fails one run and leaves the parent
  running.
- Every guest effect is visible at the parent broker, so existing approval,
  hook, redaction, and audit paths apply without a second policy engine.
- The repository gains three crates, an out-of-workspace package, a new
  protocol, and a supervised process boundary. The helper must be installed,
  packaged, and gated alongside the main binary.
- `JobContext` carries an optional authority handle, and callers that do not
  issue one cannot use `exec_code`.

## Related decisions

- [RFC 0005: Monty-based Python code execution
  environment](../rfcs/rfc-0005-monty-code-execution-environment.md).
- [ADR 0015: Audited unsafe boundaries](adr-0015-audited-unsafe-boundaries.md),
  which keeps the parent free of `unsafe` and makes the helper's self-imposed
  limits the safe route to resource enforcement.
- [Formal verification methods in
  Axinite](../formal-verification-methods-in-axinite.md), which records the
  Verus proof and its scope.
