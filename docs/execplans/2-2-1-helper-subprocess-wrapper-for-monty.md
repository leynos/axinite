# Add an isolated Monty helper and `exec_code`

This ExecPlan is a living document. Its `Constraints`,
`Tolerances (exception triggers)`, `Risks`, `Progress`,
`Surprises & discoveries`, `Decision log`, `Outcomes & retrospective`,
`Conformance basis`, and `Verification plan` sections must be kept current
during delivery.

Status: DRAFT — awaiting explicit approval

## Purpose / big picture

Roadmap item 2.2.1 gives Axinite a constrained Python codemode for short,
tool-oriented automation. A caller will be able to submit `exec_code` with
source, JSON parameters and state, and an explicit allowed-tool list. Monty
will run in a child helper process, while the Axinite parent retains ownership
of the registry, policy, approval decisions, credentials, redaction, and final
result scan. A Monty panic must fail one run without terminating the parent or
widening what the guest can ask the host to do.

Success is observable when a real helper can run a JSON-only program, request a
permitted tool through a parent broker, reject a tool outside that run's
allowlist without side effects, and return a bounded structured result. A
deliberately panicking or crashing helper must produce a run failure while a
subsequent run still succeeds in the same parent process.

This is the proposed design for review, not authorization to implement. The
plan must be explicitly approved before implementation begins.

## Approval gates

- **Plan approval.** A reviewer approves this plan, including the process
  boundary, approval denial contract, JSON protocol, provisional limits, and
  selective Verus proof, before any implementation starts.
- **Milestone acceptance.** Each implementation milestone ends in a coherent
  state with its targeted evidence and sequential repository gates recorded.
  Once the plan is approved, proceed through ordinary findings within its
  tolerances. Renew approval only for a material scope, architecture, trust,
  dependency, or proof-design deviation.
- **Delivery acceptance.** The complete feature is user-observable, the
  required documentation is synchronized, and roadmap item 2.2.1 is marked
  done. Items 2.2.2–2.2.5 remain open.
- **Publication.** After the plan is ready for review, publish it on the
  requested branch and create a draft pull request titled
  `Plan: (2.2.1) Add an isolated Monty helper and exec_code`. Its body links to
  this plan with a branch-wise link and ends with `## References` containing
  `https://lody.ai/leynos/sessions/${LODY_SESSION_ID}`. Read back the body and
  preserve the resulting Lody URL in the PR references.

## Constraints

- RFC 0005 is the governing design. Monty runs out of process; the helper
  never executes Axinite tools directly. The parent owns approvals, policy,
  credentials, and effects.
- Every run has an explicit, immutable allowed-tool set. At each callback the
  parent checks the function name against that set before invoking any tool.
  Capability selection narrows caller authority; it never expands it.
- Do not call `ToolRegistry::retain_only` to implement run-local selection:
  `retain_only(&[])` is a no-op that retains every tool, and mutating a shared
  registry would leak one run's selection into other runs. Project definitions
  from a local selected subset instead.
- Preserve caller-specific tool preflight, hooks, approval, argument
  redaction, and result redaction. `execute_tool_with_safety` validates,
  redacts parameters in logs, applies a timeout, invokes a tool, and serializes
  its output; it does not itself establish caller approval policy or guarantee
  output-leak scanning.
- Current chat approval is a dispatcher continuation: it returns
  `LoopOutcome::NeedApproval` with deferred tool calls. It is not an awaitable
  operation inside `Tool::execute`. For v1, a nested Monty callback that
  requires interactive approval must fail closed with a typed
  `approval_required`/`tool_error` and zero dispatch. A caller's approval of
  the outer `exec_code` call does not approve nested tool calls. Only tools
  that are `Never` or already explicitly approved under the caller's existing
  policy can execute. Worker/non-interactive paths retain their existing remote
  rejection. Do not implement continuation-manager plumbing in this scope; that
  requires a revised plan and likely a design decision record.
- The dispatcher must pass an immutable caller-attenuated ceiling into the
  `exec_code` invocation. `reason_ctx.available_tools` currently attenuates
  advertised definitions, but the dispatcher preflight lookup does not enforce
  that set by itself. `JobContext` does not contain session/skills policy.
  Therefore the broker must not infer authority from the global registry or
  generic `JobContext` metadata. Use an explicit invocation interface that
  carries `BrokerContext` (or an equally clear immutable value) from the
  caller. Absent or invalid caller-issued `BrokerContext` must reject before
  spawning a helper or invoking a callback. A bare `Tool::execute` invocation
  has no authority by default; add a zero-spawn/zero-effects regression.
- At selection, pin the selected `Arc<dyn Tool>` identity as well as its name.
  Before dispatch, verify the current registration is still that selected tool.
  Refactor the executor to accept the already resolved tool so a name lookup
  cannot race and substitute a different implementation. Update the internal
  caller atomically; do not add a compatibility wrapper.
- The child protocol is versioned, JSON-only, framed, bounded, and carried over
  pipes. No shell command construction, inherited credentials, raw filesystem
  access, network access, streams, bytes, callbacks to arbitrary host objects,
  or ambient environment are exposed to guest code.
- Process separation is panic and availability isolation; it is not an
  operating-system sandbox. Apply Monty's own heap, stack, allocation, and
  execution limits, plus parent-enforced wall deadline, callback count, and
  cumulative output caps. The helper is available only on platforms where the
  project can enforce its stated process limits; no unbounded fallback is
  allowed. Document Linux as the first supported deployment platform unless M1
  establishes equivalent enforcement elsewhere.
- JSON numbers must preserve the supported range and precision. Reject
  out-of-range integers, non-finite numbers (`NaN`/infinity), excessive
  nesting, and values that cannot round-trip through the chosen Monty/serde
  JSON ABI; never silently round or stringify them.
- Tool-stub generation validates Python identifiers and reserved names,
  escapes descriptions before emitting docstrings, preserves omitted optional
  arguments distinctly from explicit `null`, rejects unsupported schemas, and
  validates keyword arguments strictly against the advertised definition.
  Reject unknown requested tool names at run setup instead of silently
  shrinking the caller's explicit request.
- A parent-side supervisor owns each child and its concurrency permit until
  the child has actually exited and been waited/reaped. Cancellation cleanup
  must be RAII-backed so dropping the outer future still signals the process.
  Cap source/frame sizes before allocating their full buffers. Never replay a
  callback or whole run automatically: committed host effects cannot be rolled
  back.
- Resolve the helper executable by a trusted absolute path and spawn it without
  a shell. Clear its environment, drain and bound stderr, and terminate and
  reap it on timeout, cancellation, protocol failure, or parent shutdown.
- Keep the feature within roadmap item 2.2.1: helper process, broker, and a
  minimal usable `exec_code` JSON contract. Do not implement saved scripts,
  durable snapshots, a persistent REPL, broad audit-history features, or
  routine/workflow integration. Leave roadmap items 2.2.2–2.2.5 open.
- Review `FEATURE_PARITY.md`; update it in this branch if implementation
  changes a tracked feature's status.
- Do not introduce compatibility shims for private or pre-release APIs.
  Update affected internal callers atomically. Preserve only a real deployed
  wire-format commitment if one is found.
- Use en-GB-oxendict spelling in repository prose. Keep each source file below
  400 lines; split cohesive helper or test modules when needed.

## Tolerances (exception triggers)

- **Scope:** keep changes within the helper package, shared protocol crate,
  parent process/broker, caller invocation context, tests, proof tooling, and
  directly relevant documentation. If implementation expands into durable
  approval continuations, snapshots, saved scripts, or broad audit history,
  stop and present a revised scope. Use repository ownership as the boundary
  rather than arbitrary line counts.
- **Security boundary:** if Monty requires in-process execution, ambient
  environment, direct tool access, or a protocol that cannot be bounded, stop.
- **Resource enforcement:** if the target platform cannot enforce configured
  process/memory limits, the feature is unavailable there. Do not claim that
  panic isolation alone constrains memory or CPU, and do not fall back to
  unbounded execution.
- **Approval:** v1 must not suspend chat dispatch for an interactive approval.
  Callbacks that require approval fail closed before dispatch. If useful
  workflows require in-run approval, record this as a scope/design escalation
  for a continuation manager rather than quietly expanding 2.2.1.
- **External API/dependency:** a new runtime dependency is expected for Monty
  in the helper package only. If the compatible published Monty version, API,
  feature set, or licensing cannot be established, stop before adding it. Add
  the requested `googletest` dev-dependency; reuse the existing
  `pretty_assertions`, `rstest`, `rstest-bdd`, `insta`, and `proptest` versions.
- **Wire/protocol:** if real Monty callback/resume semantics cannot fit the
  versioned JSON protocol, or if preserving a deployed peer format requires
  compatibility support, stop and record options for review.
- **Limits:** initial review values are proposed defaults for approval: source
  up to 64 KiB, each protocol frame up to 1 MiB, captured stderr up to 64 KiB
  per run, a 30-second total wall deadline, at most four simultaneous helpers
  per parent, at most 64 callback rounds, and at most 4 MiB cumulative
  callback/result JSON per run. Validate these against current worker execution
  limits and meaningful use cases. Also select the proposed Monty heap,
  allocation, and stack limits against the pinned API. Escalate if those values
  conflict with existing operational limits or make the target workflows
  unusable; do not silently raise them.
- **Verification:** if a small pure capability-projection kernel cannot be
  proved in Verus without assuming its postcondition or introducing trusted
  bodies, stop and report the exact gap. Do not weaken the property to obtain a
  green proof.
- **Quality gates:** when a gate fails, inspect its captured log, fix the
  cause, then rerun the affected gate sequence. Stop after three consecutive
  failures with the same underlying cause and report the evidence.
- **Architecture:** any need for persistent state, saved scripts, a general
  Python package surface, or an unreviewed trust boundary is outside tolerance
  and requires a plan revision and approval before continuing.

## Risks

- **Upstream Monty API drift.** Severity high; likelihood medium. RFC 0005
  describes Monty as experimental; its current release maturity and APIs need
  verification. In M1, check the exact published crate release and documented
  Rust API, then pin the selected compatible version with a caret requirement
  and lockfile. Keep Monty linked only by the helper package. Do not install a
  Python runtime or package.
- **Subprocess protocol complexity.** Severity high; likelihood medium. A child
  can panic, exit, truncate output, or send malformed data. Use explicit frame
  lengths, protocol version and message discriminants, bounded reads, one
  terminal outcome, and unconditional child cleanup; exercise the actual
  process boundary in system tests.
- **Approval behaviour differs by caller.** Severity high; likelihood medium.
  Trace chat tool preflight, hooks, approval, and post-flight result scanning.
  Name supported `exec_code` callers. Reject approval-required callbacks before
  dispatch rather than falling back to direct execution.
- **Result leakage.** Severity high; likelihood medium. Tool output may contain
  sensitive material even if argument logging is redacted. Scan/redact in the
  parent before serialization to the helper and before returning final output;
  test a known sensitive marker at both boundaries.
- **Resource limits are not portable.** Severity high; likelihood medium.
  Parent deadlines and Monty limits do not imply operating-system memory
  enforcement. Select a Linux process-limit mechanism and make unsupported
  platforms unavailable; document the supported build/deployment matrix.
- **JSON number fidelity.** Severity medium; likelihood medium. Monty and
  serde_json may not share every numeric range; test boundaries and reject
  values that cannot round-trip exactly.
- **Generated stubs cross a code boundary.** Severity high; likelihood medium.
  Host tool names and descriptions must not become executable Python syntax or
  injected docstrings. Validate identifiers, escape text, and reject invalid
  schemas before helper launch.
- **Default environment leakage.** Severity high; likelihood low. Spawn with a
  cleared environment, explicit working directory policy, no inherited file
  descriptors except protocol pipes, and no shell. Verify with a helper probe
  that expected ambient values are absent.
- **Proof/runtime refinement is unclear.** Severity high; likelihood medium.
  Prefer using the verified executable projection kernel in production. If
  Verus requires a proof-only representation, M1 must establish a deterministic
  generated artefact or another rigorous refinement bridge and drift check.
  Handwritten mirror plus differential tests is not an exhaustive proof that
  production code implements the verified function. If no credible bridge is
  practical, block the proof design for review instead of claiming the shipped
  attenuation is formally verified.
- **Callback registration changes during a run.** Severity high; likelihood
  low. Pin the selected `Arc<dyn Tool>` identity and compare it with the
  current registry entry immediately before dispatch; invoke the resolved
  object rather than looking up by name again.
- **Cancellation leaks a helper or admission permit.** Severity high;
  likelihood medium. The supervisor/RAII owner retains both child and permit
  through actual wait/reap, including when the parent future is dropped.
- **Build and proof-tool availability.** Severity medium; likelihood medium.
  The repository documentation describes a Verus workflow, but this checkout
  does not yet contain its scripts or pinned tool bundle. M1 must settle
  reproducible `rust-prover-tools` pin/install/run commands and keep proof
  setup separate from ordinary Cargo gates.

## Progress

- [x] (2026-10-20 19:25Z) Read the RFC 0005 contract, roadmap item 2.2.1,
  repository rules, available runtime seams, and ExecPlan requirements.
- [x] (2026-10-20 19:25Z) Drafted a proof-sketch-first design, milestones,
  test strategy, operational boundaries, and review tolerances.
- [x] (2026-10-20 19:25Z) Reconciled the approval contract, caller-attenuated
  broker context, tool identity pinning, process cleanup, and proof-refinement
  gate with runtime evidence.
- [ ] Obtain explicit approval of this plan. No implementation has started.
- [ ] M1: verify Monty and approval-flow feasibility; update this plan with
  evidence and final protocol/limits before coding.
- [ ] M2–M5: implement and validate the approved milestones.
- [ ] Update docs and mark roadmap 2.2.1 done when the feature is complete.

## Surprises & discoveries

- Observation: `ToolRegistry::retain_only` treats an empty name list as a
  no-op, so it cannot safely encode “no tools” and mutates shared state.
  Evidence: `src/tools/registry/loader.rs`, `retain_only`. Impact: the run
  capability view must be a separate immutable projection.
- Observation: `execute_tool_with_safety` does not itself own caller-specific
  approval or final output leak scanning. Evidence: `src/tools/execute.rs` and
  chat preflight in `src/agent/dispatcher/delegate/tool_exec/preflight.rs`.
  Impact: the parent broker must preserve the complete caller path rather than
  treating this helper as a universal safety wrapper.
- Observation: chat approval is represented by dispatcher continuation
  `LoopOutcome::NeedApproval` and deferred tool calls; it cannot be awaited
  inside `Tool::execute`. Also, the worker's local remote proxy has a `Never`
  requirement while the orchestrator rechecks the real tool and returns 403
  when approval is needed. Evidence: runtime source/review context. Impact: v1
  rejects an interactive-approval callback before dispatch; outer approval
  never authorizes nested effects.
- Observation: `Tool::execute(params, &JobContext)` receives a `JobContext`
  without session or skills policy, and advertised `reason_ctx.available_tools`
  are not themselves enforced by dispatcher preflight lookup. Evidence:
  `src/tools/tool/traits.rs:129-158`, `src/context/state.rs`, dispatcher loops,
  and remote tool recheck. Impact: `exec_code` must receive explicit immutable
  `BrokerContext` from the attenuating caller and validate tool identity at
  dispatch; global registry and generic job metadata cannot define authority.
- Observation: helper and concurrency permit lifetime must outlive cancellation
  of the outer future until actual child exit/reap. Evidence: process lifecycle
  analysis. Impact: supervisor-owned RAII cancellation is part of the
  subprocess adapter contract; no automatic replay can compensate for committed
  host effects.
- Observation: a handwritten Verus mirror with differential tests does not
  establish exhaustive refinement to production selection. Evidence: proof
  review. Impact: use the verified projection in production or establish a
  deterministic artefact/refinement bridge; otherwise stop at M1 for a design
  decision.
- Observation: the formal verification design document describes a pinned
  Verus installation and `make verus`, but this checkout lacks those scripts
  and tool files. Evidence: `docs/formal-verification-methods-in-axinite.md`
  versus absent `scripts/run-verus.sh`, `scripts/install-verus.sh`, and
  `tools/verus/`. Impact: M1 must specify the minimal pinned runner before the
  planned proof milestone can be accepted.
- Observation: upstream Monty has both direct execution (`MontyRun::new` and
  `run`) and resumable execution (`start`, callback progress, and resume
  methods); its host integration must enforce its own suspension limits.
  Evidence: official Monty Rust quickstart and current API research dated
  2026-10-20. Impact: confirm the pinned release's exact API during M1; do not
  assume callbacks are automatically bounded by a pool.

## Decision log

- Decision: isolate all Monty interpreter linkage in a helper executable and
  exchange bounded, versioned JSON frames over stdin/stdout pipes. Rationale: a
  Rust panic is process-local; the parent remains responsible for capabilities
  and policy. RFC 0005 recommends a dedicated helper. Date/Author: 2026-10-20,
  plan author.
- Decision: derive an immutable selected-tool view for each run and re-check
  every callback name at the parent boundary. Rationale: registry contents are
  shared, and empty `retain_only` is a no-op. Date/Author: 2026-10-20, plan
  author.
- Decision: v1 rejects callbacks requiring interactive approval with a typed
  denial before dispatch; it supports only effects already eligible under the
  caller's existing non-interactive/explicitly-approved policy. Rationale: chat
  approval returns a dispatcher continuation rather than an awaitable from
  `Tool::execute`; outer approval is not nested approval. A continuation
  manager is a larger surface requiring separate design review. Date/Author:
  2026-10-20, proposed for explicit plan approval.
- Decision: pass an explicit immutable `BrokerContext` from the attenuating
  caller and pin selected tool object identities through dispatch. Rationale:
  `JobContext` lacks session/skills policy, advertised available tools are not
  enforced by preflight alone, and a second name lookup can race registry
  replacement. Refactor the internal executor atomically. Date/Author:
  2026-10-20, proposed for explicit plan approval.
- Decision: parent supervisor owns child and permit until wait/reap; RAII drop
  cleanup kills the child; side effects are never automatically replayed.
  Rationale: dropping an async future must not orphan a process, and an already
  committed tool effect cannot be rolled back. Date/Author: 2026-10-20,
  proposed.
- Decision: Verus must prove the exact projection used by production, by shared
  executable kernel or a rigorous deterministic refinement bridge. If M1 cannot
  establish this without trusted bodies or an assumed postcondition, block and
  ask for design direction. Rationale: the security claim concerns shipped
  attenuation; a proof-only handwritten mirror plus sampled differential tests
  cannot discharge it. Verus does not apply to protocol diagnostics, subprocess
  lifecycle, or approval integration, which remain covered by tests.
  Date/Author: 2026-10-20, proposed.
- Decision: this plan covers only 2.2.1 and its minimum JSON boundary needed
  to use `exec_code`; saved scripts, expanded audit history, and workflow
  integrations stay in 2.2.2–2.2.5. Rationale: preserve roadmap decomposition
  while delivering a meaningful usable first operation. Date/Author:
  2026-10-20, plan author.

## Outcomes & retrospective

Draft only. No implementation outcome exists yet. At each plateau record the
observable result, remaining limitations, gate transcripts, deviations, and
lessons here. At completion reconcile any discoveries against RFC 0005 and
roadmap 2.2.1 before marking this plan complete.

## Context and orientation

Start with `AGENTS.md`, `docs/contents.md`, `docs/welcome-to-axinite.md`, and
`docs/axinite-architecture-overview.md`. The user-requested implementation
references are `docs/rust-testing-with-rstest-fixtures.md`,
`docs/reliable-testing-in-rust-via-dependency-injection.md`,
`docs/rust-doctest-dry-guide.md`,
`docs/complexity-antipatterns-and-refactoring-strategies.md`, and
`docs/rstest-bdd-users-guide.md`. Also read `src/tools/README.md` for tool
ownership and
`src/agent/dispatcher/tests/features/active_skill_context.feature` for
repository BDD conventions. Consult `docs/developers-guide.md`,
`docs/users-guide.md`, `docs/tool-calling-architecture.md`, and
`docs/rfcs/0005-monty-code-execution-environment.md` during implementation.

The relevant implementation seams are:

- `src/tools/execute.rs`: common parameter validation, sensitive-argument log
  redaction, per-tool timeout, execution and JSON serialization.
- `src/tools/registry/loader.rs`: registry projection and tool definitions.
  Do not use shared-registry mutation to represent one run's authority.
- `src/agent/dispatcher/delegate/tool_exec/preflight.rs` and the surrounding
  `src/agent/dispatcher/delegate/tool_exec/` modules: chat approval preflight,
  hook and tool execution flow, and result handling. Trace the full call path
  before choosing how `exec_code` enters it.
- `src/tools/`: tool trait, built-in registration, and user-facing tool
  adapters. Keep the Monty interpreter out of this process.
- Root `Cargo.toml` and `Cargo.lock`: parent runtime dependencies. The helper
  package should own the Monty dependency if Cargo packaging permits it.
- `docs/rfcs/0005-monty-code-execution-environment.md`: design authority for
  JSON values, broker behaviour, errors, process boundary, and rollout.

“Capability projection” means constructing a run-local set of tool names and
definitions from the intersection of caller authority, the explicit requested
allowlist, and currently available tools. “Terminal outcome” means the one
final result of a run: completed, script/tool error, resource limit, timeout,
cancellation, or runtime crash.

The design is informed by the official Monty Rust quickstart at
<https://pydantic.dev/docs/monty/quickstart/rust/>. It documents synchronous
and resumable execution APIs, including progress events for external calls. The
upstream `monty-pool` project was also considered as subprocess prior art; its
protobuf protocol does not match RFC 0005's JSON-only parent/child contract.
The selected design retains an Axinite-owned JSON protocol unless review
evidence changes the RFC. These findings were checked via Firecrawl on
2026-10-20; M1 must re-verify the selected published crate release and API.

Load these skills during implementation: `codegraph-mcp` for semantic
navigation and impact checks; `hexagonal-architecture` to keep tool policy
inside application/domain boundaries and subprocess mechanics in an adapter;
`rust-router`, then the smallest relevant follow-on skills for API shape,
errors, asynchronous cancellation, testing, and verification; `execplans` to
maintain this plan; and `firecrawl-mcp` if external API/protocol evidence
changes. `rust-errors`, `rust-async-and-concurrency`, `rust-types-and-apis`,
`rust-unit-testing`, `rust-verification`, and `verus` are the expected focused
Rust follow-ons.

## Conformance basis

- **Baseline:** repository revision
  `9290dbc191d928cab6182904582491fc35dfb33e`; all local references below were
  inspected against this revision.
- **Roadmap:** `docs/roadmap.md`, phase 2 objective and task 2.2.1. No separate
  Terms of Reference is present for this task.
- **Design:** proposed RFC 0005, dated 2026-03-11; especially `Summary`,
  `Public operations`, `Guest/Host ABI`, `Tool Broker Design`,
  `Isolation Model`, `Safety Model`, and `Rollout Plan`. The RFC has status
  Proposed; this plan treats it as governing pending reviewer approval.
- **Architecture:** `docs/axinite-architecture-overview.md`, current runtime
  shape; `docs/tool-calling-architecture.md`, existing mediated tool path.
- **Testing and verification standards:** `AGENTS.md`; the six required Rust
  testing references listed above;
  `docs/formal-verification-methods-in-axinite.md`.
- **Design elements:** `EP-DE-1` parent/child separation; `EP-DE-2` immutable
  per-run capability projection; `EP-DE-3` caller-policy and approval
  preservation; `EP-DE-4` bounded JSON protocol and child lifecycle; `EP-DE-5`
  selective Verus proof of the pure projection kernel.

Traceability:

- `ROADMAP-2.2.1` (helper isolates panic; callbacks limited to explicit
  per-run allowlist) -> `RFC0005-ISOLATION`, `RFC0005-BROKER`,
  `RFC0005-JSON-ABI` -> `EP-DE-1..5` -> `EP-M1..M5` -> process, capability,
  approval, and result evidence named below.
- `RFC0005-SAFETY` (parent retains policy, approval, credentials and tool
  execution) -> `EP-DE-2..4` -> `EP-M2..M4` -> denial, approval, leak-scan,
  crash-isolation, and recovery tests.
- Baseline gap: no Monty `exec_code` tool or helper exists. -> `EP-M2..M4` ->
  visible invocation and structured result from the real process.
- Baseline gap: the current shared registry is unsuitable as a run-local
  allowlist. -> `EP-DE-2`, `EP-DE-5` -> `EP-M2`, `EP-M3` -> proof and
  property-test evidence for selection and non-widening.
- Roadmap 2.2.2–2.2.5 are dependencies or follow-on capabilities, not
  acceptance criteria here. A minimum JSON envelope is required to make
  `exec_code` useful; persisted state, saved scripts, broad audit metadata, and
  higher-level workflows remain deferred.

## Verification plan

### Proof sketch before executable design

Define sets independently of the proposed code:

- `A` is the caller's granted authority set after existing attenuation. `A`
  is the actual set granted to this invocation, not a broader upper bound.
- `R` is the explicit allowlist supplied for this run. It is required; an
  empty set means no callable tools.
- `V` is the set of tools currently available to the caller.
- `S = A ∩ R ∩ V`, with the reserved `exec_code` capability removed. The first
  implementation denies direct and indirect recursion.
- A callback request is `(run_id, call_id, name, arguments)`. The broker may
  begin execution only if `name ∈ S`, arguments pass the existing caller path,
  and existing policy says interactive approval is not required or the tool is
  already explicitly approved. A callback needing interactive approval returns
  a typed denial before dispatch. Results cross back only after JSON validation
  and the caller's output redaction/leak scan.
- A run has a monotonic phase:
  `Created -> Running -> WaitingForHost* -> Running -> exactly one Terminal`.
  For v1, interactive approval is a terminal denial, not a suspended phase.
  Cancellation, deadline, denial, protocol failure, helper crash, and
  completion cannot transition out of Terminal.

Required preconditions: explicit `R`; `A` is the caller's granted authority;
`V` is a snapshot of callable tools at run start; every callback is revalidated
against immutable `S`; selected caller policy remains available; and tool
object identities are pinned. Immediately before dispatch, verify that the
current registry entry is still the pinned object and invoke that resolved
object rather than looking it up again by name.

Required postconditions: returned tool calls are all members of `S`; no
unselected callback starts; an empty `S` denies every callback; one host
callback identity cannot be executed twice by one request/resume cycle unless
Monty emits a distinct request; every run ends at most once; and a helper crash
cannot terminate the parent process.

Required semantic lemmas, treated as hypotheses about useful proof structure:

1. **Projection exactness:** the implementation emits exactly the names
   present in `A ∩ R ∩ V`, excluding reserved recursive capabilities.
2. **Non-widening:** `S ⊆ A`, `S ⊆ R`, and `S ⊆ V`; therefore intersection
   never grants authority absent from any input set.
3. **Empty-denies-all:** if any required set intersection is empty, then `S`
   is empty, and membership checking permits no callback.
4. **Callback gate soundness:** every tool side effect has a prior successful
   `name ∈ S` check and caller-policy success; approval-required callbacks fail
   before dispatch and outer approval does not grant nested authority.
5. **Terminal uniqueness and monotonicity:** no event after terminal changes
   the outcome or starts another callback.
6. **Process isolation:** a child exit or panic is observed as a failed run
   while the parent remains able to service another run. This is an OS/process
   integration claim, verified by tests rather than Verus.

The proof design pressures the API toward an ordered pure projection function
returning selected entries (names and pinned object identities), followed by a
separate runtime gate that checks every callback. Production must use this
verified executable kernel, or M1 must establish a deterministic generated
artefact/refinement bridge with a drift check. A handwritten proof mirror plus
differential tests is insufficient. Do not assume input names are unique:
validate uniqueness at the adapter boundary or make ordered duplicate semantics
explicit in the kernel. If the production bridge is not credible, block design
approval rather than claim the shipped code is formally proved.

### Obligations and evidence

- **`VFY-CAP-PROJECTION`: exact, non-widening capability selection for
  arbitrary-length finite ordered entries.** Method: Verus on the same
  executable kernel production uses, proposed at `verus/monty_capabilities.rs`,
  plus `proptest` properties at `src/tools/monty/capabilities/tests.rs`. A
  shared protocol crate owns the serializable request/response types, not
  interpreter code. Command: `make verus-monty-capabilities` and
  `cargo nextest run -p axinite --lib monty::capabilities::tests`. Evidence:
  Verus establishes output exactness and subset properties for all admitted
  inputs; the test checks production entries retain their pinned identity and
  match a separately specified mathematical selection. Non-vacuity: exercise
  empty sets, each singleton partition, duplicates, unknown names, reserved
  recursive name, and generated non-empty cases; assert generators report
  classification counts and include witnesses for each class. Seed a test-only
  mutation that omits one input set and require the property test to reject it.
  Also seed an always-empty-output mutation and require non-empty witnesses to
  reject it. The proof must use a substantive loop invariant stating that the
  ordered output after processing prefix `i` equals exactly the eligible
  selected entries from the first `i` requested entries; derive exactness,
  non-widening, and empty-denies-all from this invariant. No `assume`, `admit`,
  external proof bodies, or conclusion-as-premise. Proof scope is the pure
  projection only; it does not prove OS, registry, protocol, policy, or tool
  behaviour.
- **`VFY-CALLBACK-GATE`: a callback outside `S` causes no tool invocation and
  every invocation rechecks set membership and pinned tool identity.** Method:
  `rstest`, BDD behaviour, and integration side-effect counter. Artefacts:
  `src/tools/monty/capabilities/tests.rs`, `src/tools/monty/broker/tests.rs`,
  `src/tools/monty/features/exec_code.feature`. Future focused commands:
  `cargo nextest run -p axinite --lib monty::capabilities::tests` and
  `cargo nextest run -p axinite --lib monty::broker::tests`. Expected evidence:
  disallowed and empty-allowlist scenarios leave the counter at zero; allowed
  calls invoke the pinned tool once; replaced registration is rejected. A
  negative control with membership or identity validation removed must fail.
- **`VFY-APPROVAL-DENIAL`: a nested callback needing interactive approval
  yields typed denial before dispatch, and outer `exec_code` approval does not
  authorize the nested effect.** Method: `rstest` and BDD behaviour; no formal
  proof because this depends on caller policy. Evidence: a `Never` callback
  succeeds, an unapproved interactive callback is denied with zero side
  effects, an explicitly pre-approved call follows existing policy, and worker
  remote rejection remains intact. A negative control treating outer approval
  as nested approval must fail.
- **`VFY-JSON-FRAMES`: frames are versioned and rejected when malformed,
  oversized, truncated, or of an unknown message kind; only JSON values pass
  the guest boundary.** Method: `rstest` finite cases and `proptest` over
  arbitrary frame bytes / JSON values. Evidence: valid request/callback/result
  round trips; malformed and oversized frames terminate the run with typed
  protocol failure; property generation covers all JSON primitive and compound
  variants plus invalid UTF-8 and length boundaries. Include a negative control
  that bypasses the frame-size check and show the boundary test fails. Do not
  use Verus for codec and diagnostic formatting.
- **`VFY-TERMINAL-LIFECYCLE`: each run records at most one terminal result and
  all terminal paths clean up the helper.** Method: `rstest` transition cases
  and real subprocess behavioural tests. Evidence covers completion, script
  error, helper panic/exit, timeout, cancellation, malformed frame, and parent
  shutdown. A later run after each child failure must succeed. The executable
  witness must reach each terminal branch; no artificial event-log proof is
  required.
- **`VFY-REDACTION`: raw sensitive arguments/results do not appear in logs,
  approval display, helper-returned values, errors, or final result sinks.**
  Method: integration tests with unique sentinel secrets and snapshot tests
  (`insta`) for stable error variants. Evidence scans captured output at each
  boundary and requires no sentinel occurrence; a deliberately disabled result
  scan must fail. Snapshot only stable structured output, not timestamps,
  paths, or nondeterministic process details.
- **`VFY-PROCESS-ISOLATION`: Monty panic cannot terminate the Axinite parent.**
  Method: real executable end-to-end test using the same IPC adapter, where a
  test helper mode panics/exits and a subsequent valid run succeeds in the same
  parent test process. This is an OS contract assumption exercised on CI
  platforms. Record platforms and any skip explicitly. Do not claim Verus
  proves child process isolation.

External assumptions are limited to the selected Monty Rust API honoring its
documented `start`/progress/resume contracts, the operating system delivering
child exit and pipe EOF, and Tokio process cancellation/kill behaviour. The
project verifies its owned protocol, projection, lifecycle, and policy
integration against the real helper interface; it does not verify Monty
internals. Formal verification is warranted only for the simple pure capability
projection, whose all-input subset property is a security contract. The async
policy loop and process lifecycle use executable tests because their
correctness depends on runtime and external process behaviour.

## Plan of work

### Stage A — evidence and proposed API, no code changes

Trace `exec_code` registration and invocation via CodeGraph, then verify the
caller-specific tool preflight, hooks, approval decision, redaction, and
post-flight leak scan. Confirm how `reason_ctx.available_tools` is derived and
why dispatcher preflight lookup does not itself enforce that set. Inspect the
feature-gated worker/container execution boundary to identify where the helper
belongs. Check current Monty crate metadata, exact Rust APIs, licence, feature
dependencies, and supported targets against official documentation and
published crate metadata. Compare RFC 0005 with `monty-pool` prior art and
retain the JSON contract unless approved evidence changes it. Document which
callers can pass immutable attenuated `BrokerContext`, approval-required
denial, return shape, errors, protocol, resource enforcement, and helper
resolution.

**Go/no-go:** proceed only if a published, versionable Monty crate supports the
host-callback loop in an isolated helper, production authority projection can
be rigorously connected to the Verus kernel, explicit `BrokerContext` reaches
invocation, and supported targets enforce resource limits. Otherwise record the
material design change for review; do not implement a fallback.

### Stage B — red specifications and proof artefact

Add the smallest failing tests before production code. Define unit tests for
projection and frame limits, one BDD feature for the user-visible broker flow,
and a real-helper integration harness with a test helper mode for panic/exit.
The Gherkin specification must include:

```gherkin
Feature: Execute bounded Python code through the host tool broker

  Scenario: An allowed tool call returns JSON to the code
    Given a caller grants the run the "lookup" tool
    And the helper program requests "lookup" with JSON arguments
    When the caller executes the code
    Then the parent invokes "lookup" through its normal policy path
    And the code receives only the redacted JSON result

  Scenario: A tool outside the run allowlist has no side effect
    Given the caller grants no tools to the run
    When the helper program requests "lookup"
    Then the callback is denied before invocation
    And the tool side-effect count remains zero

  Scenario: A helper panic does not stop the parent runtime
    Given a Monty helper run panics
    When the parent reports the failed run
    And the caller starts a second valid run
    Then the second run completes successfully

  Scenario: Approval-required nested callback is denied before dispatch
    Given the requested allowed tool requires interactive approval
    When the parent checks the caller's current policy
    Then the tool side-effect count remains zero
    And the code run ends with a typed approval-required denial
```

Use `rstest` fixtures and parameter cases for normal, rejected, malformed,
oversized, cancellation, and error inputs. Use the requested `googletest` and
`pretty_assertions` assertions for clear failure semantics. Add `proptest` for
capability projection and JSON/frame invariants. Use `insta` only for stable
multivariant error/result JSON. Red tests must fail for the expected reason
before implementation. Build the proof sketch into the candidate Verus module
and confirm the initial open/failing obligation is caused by missing
implementation, not an inconsistent precondition.

### Stage C — helper protocol and pure capability core

Create `crates/monty-helper` for the executable and `crates/monty-protocol` for
versioned request/response types and bounded framing. Only the helper links
Monty; the parent depends on protocol types, never the interpreter. Parent
starts it with explicit pipes, cleared environment, trusted path, no shell,
bounded stderr drain, and cleanup on every exit. The helper accepts code and
JSON, starts Monty, yields host-call requests, accepts JSON resume values, and
returns one typed terminal result. It never knows the registry, approvals,
credentials, or actual tools.

Add parent modules `src/tools/monty/runner.rs` (supervisor),
`src/tools/monty/broker.rs`, `src/tools/monty/capabilities.rs`, and
`src/tools/monty/exec_code.rs`. Put the pure projection kernel in a crate
usable by both production and Verus without importing Monty. Implement its
proof in `verus/monty_capabilities.rs`. In M3 add the repository-standard
pinned verifier artefacts `tools/rust-prover-tools/REF`, `tools/verus/VERSION`,
and `tools/verus/SHA256SUMS`, plus a Make target that invokes
`rust-prover-tools` to install/run Verus. Do not add bespoke shell wrappers. M1
records the supported tool reference and official install invocation. If
production cannot use the verified kernel and no rigorous deterministic
refinement bridge exists, block the proof design.

### Stage D — parent broker and public `exec_code`

Expose
`exec_code(code, allowed_tools, params=None, state=None,
entrypoint="main", limits=None)`
through an invocation interface carrying immutable `BrokerContext` from the
attenuating caller. Normalize omitted `params` and `state` to empty objects;
require `allowed_tools`, with empty meaning no tools. Reject unknown requested
names. Validate `entrypoint` as a permitted Python identifier and run the RFC
async wrapper:

```python
async def main(params, state):
    ...

await main(params, state)
```

Require an object result containing object `state`. Check callback membership
and pinned identity, then run preflight/hooks against the final dispatched name
and arguments. If a hook rewrites the request, repeat allowlist, schema,
identity, and approval checks against the rewritten call. Reject interactive
approval before dispatch: outer `exec_code` approval never authorizes nested
effects. Reject recursion, redact/scan results, normalize JSON, and resume the
helper. Kill/reap on cancellation; never replay effects.

This milestone must demonstrate a real useful round trip: input JSON reaches
the program, an explicitly allowed tool is invoked, filtered JSON data returns
to the program, and its JSON object result/state returns to the caller. It does
not provide saved scripts, durable state, a long-lived REPL, or broad run
history.

### Stage E — documentation, refactor, and final validation

Record the process boundary, fail-closed approval policy, authority bridge, and
proof/runtime relationship in a new `docs/adr-NNN-monty-execution.md`, choosing
the next unused ADR number at implementation time. Follow the status, date,
context, options, decision, and consequences sections in
`docs/documentation-style-guide.md`. Reference it from RFC 0005 and update
`src/tools/README.md`, `docs/tool-calling-architecture.md`, and
`docs/repository-layout.md` for the new internal boundaries and re-use
strategy. Update `docs/rfcs/0005-monty-code-execution-environment.md` with any
approved protocol, limits, approval, or helper placement decisions. Update
`docs/axinite-architecture-overview.md` or the relevant tool-calling component
document with the parent/helper boundary. Update `docs/developers-guide.md` for
internal helper packaging, protocol, and proof commands. Update
`docs/users-guide.md` for the new invocation, examples, limits, and denial
behaviour. Update `docs/roadmap.md` to mark 2.2.1 done only after all
acceptance evidence passes; leave 2.2.2–2.2.5 untouched. Add the plan to
`docs/contents.md` if the index's plan listing convention includes current
plans. Run doc checks and full code gates sequentially after each major
milestone. Review nearby modules for refactoring opportunities and keep any
refactor in a separate commit with its own affected gates.

## Milestones and plateaus

### EP-M1 — confirm upstream API, authority handoff, and resource boundary

**Outcome:** record helper/protocol packages, Monty release/API, caller
invocation context, approval-denial behaviour, protocol messages, resource
enforcement, helper resolution, and limits. This evidence milestone changes no
implementation code. Plan approval authorizes ordinary discovery within
tolerance; renewed approval is needed only for a material deviation.

**Requirements/gaps:** `ROADMAP-2.2.1`, `RFC0005-ISOLATION`, `RFC0005-BROKER`,
and `EP-DE-1..5`.

**Acceptance evidence:** official Monty docs and crate metadata confirm API,
version, and licence; call graph shows preflight/result scanning; immutable
`BrokerContext` and tool identities reach invocation; helper packaging and
Linux resource enforcement are viable; production projection uses the proof
kernel or a reviewed deterministic refinement bridge. Record prover version and
official install invocation for M3.

**Conformance check:** RFC scope still fits 2.2.1; all side effects remain
parent-owned; no unapproved public interface or dependency; no new trust
boundary; trace links updated. **Recovery:** revise this plan if feasibility
fails; mark BLOCKED and obtain approval for the deviation. **Remaining gaps:**
all implementation. **Compatibility decision:** none; no deployed Monty peer
exists.

### EP-M2 — isolated helper and bounded protocol plateau

**Outcome:** parent spawns a real Monty helper, executes JSON-only code with no
callbacks, parses typed results, and recovers from malformed frames, panic,
timeout, or cancellation. Distinguish missing/version-mismatched helper,
non-zero exit or panic, and timeout; do not infer memory faults from signal
number. No tool can be invoked yet.

**Requirements/gaps:** `EP-DE-1`, `EP-DE-4`; establishes the helper needed for
`ROADMAP-2.2.1`.

**Acceptance evidence:** unit tests cover frame/version/length/error cases;
real-process tests prove run success, panic isolation, timeout kill/reap,
bounded stderr including a secret sentinel, and successful next-run recovery.
Release/container smoke test proves helper packaging. No Monty dependency is
linked into the parent. **Conformance check:** parent owns process lifecycle;
wire shape remains JSON and bounded; no guest host effects; no compatibility
layer added. **Recovery:** revert the helper package change atomically if the
process contract is unsafe; preserve the failing reproducer. **Remaining
gaps:** callbacks, caller policy, approvals, public tool. **Compatibility
decision:** none.

### EP-M3 — proved capability projection and broker callback gate plateau

**Outcome:** helper can request host functions, but parent invokes only names
selected for that immutable run and returns only validated/redacted JSON.

**Requirements/gaps:** `EP-DE-2`, `EP-DE-3`, `EP-DE-5`; callback half of
`ROADMAP-2.2.1`.

**Acceptance evidence:** Verus proof passes; proptest matches mathematical
intersection and rejects a seeded omitted-set mutation; callback tests prove
empty and out-of-set requests have zero side effects, while selected requests
use the caller path. Redaction sentinel is absent from helper input and logs.
**Conformance check:** exact projection and re-check remain at the parent;
approval, hooks, and scan are preserved; no direct tool adapter calls; proof
scope and runtime refinement are stated honestly. **Recovery:** isolate proof
or broker failure behind typed errors and resume only after fixing; do not
loosen allowlist. **Remaining gaps:** public `exec_code` workflow, final
documentation. **Compatibility decision:** none.

### EP-M4 — usable `exec_code` behaviour and caller policy plateau

**Outcome:** a caller executes a bounded program with explicit JSON inputs,
state, and allowed tools; interactive approval-required callbacks fail closed
before dispatch; result and denial contracts are visible.

**Requirements/gaps:** `ROADMAP-2.2.1`; `RFC0005-JSON-ABI`, `RFC0005-SAFETY`;
`EP-DE-1..4`.

**Acceptance evidence:** named unit tests, `rstest-bdd`, and real helper
end-to-end tests cover happy and unhappy paths, approval-required denial,
`Never` and pre-approved tools, worker remote rejection, redaction, malformed
output, resource limits, cancellation, helper crash then recovery, and
recursive-call denial. The minimal JSON ABI needed by `exec_code` works end to
end. **Conformance check:** selection cannot widen; approval and output scan
are caller-owned; no saved scripts or workflow integration; no hidden
persistent state. **Recovery:** disable the `exec_code` registration if its
caller policy cannot be enforced; retain helper and test harness as a valid
plateau if safe. **Remaining gaps:** saved scripts 2.2.3; expanded audit
metadata 2.2.4; workflow integration 2.2.5. **Compatibility decision:** none.

### EP-M5 — docs, gates, roadmap and delivery plateau

**Outcome:** feature behaviour, architecture, test/proof workflow, and limits
are documented and 2.2.1 is marked done.

**Acceptance evidence:** updated RFC/design/component docs and user/developer
guides agree with implementation; `make check-fmt`, `make lint`, `make test`,
`make spelling`, and `make markdownlint` pass; `make audit` passes if runtime
dependency manifests or lockfile changed; pinned Verus command passes. Run
gates sequentially with output captured in `/tmp` and inspect logs. Commit
after each coherent milestone and run its required gates before commit.

**Conformance check:** all roadmap requirements mapped; upstream RFC and
architecture changes recorded; feature parity checked; trace links current; no
unapproved dependency or trust boundary. **Recovery:** leave the roadmap item
open until every criterion passes. **Remaining gaps:** roadmap items
2.2.2–2.2.5. **Compatibility decision:** none.

## Concrete steps

Create the BDD scenario bindings at `tests/monty_exec_code_bdd.rs`, using the
feature file `src/tools/monty/features/exec_code.feature`; add real-process
integration tests at `tests/monty_helper_e2e.rs`. Helper package tests use
`crates/monty-helper/tests/`. Add Make targets `test-monty-bdd`,
`test-monty-e2e`, and `test-monty-helper` delegating respectively to these
commands, then use the Make targets for focused red/green/refactor runs:

```bash
cargo nextest run -p axinite --test monty_exec_code_bdd
cargo nextest run -p axinite --test monty_helper_e2e
cargo nextest run -p axinite-monty-helper
```

Name the helper package `axinite-monty-helper` and executable
`axinite-monty-helper`. The E2E target builds that executable first and passes
its absolute path through injected test configuration. Do not discover it from
the test runner's current executable or ambient PATH. A fake helper exercises
malicious frames; the actual helper exercises the pinned Monty runtime, async
callbacks, and entrypoint harness. Tests include concurrent runs with disjoint
grants, dropped outer futures, repeated timeout/cancel cycles, stale replies,
hook rewrites, missing authority, and a clean next run. Assert zero lingering
children, restored permits, and no late effects.

Release acceptance additionally stages the main binary and helper together
outside the build tree, invokes the same E2E path against that installation,
and repeats it inside the worker image. Extend the existing Cargo workspace,
install/dist configuration, Dockerfiles, and gate targets so all new packages
are built, formatted, linted, tested, and shipped. Record exact release/image
commands selected in M1; no platform skip may substitute for the Linux
acceptance run. Feature-sensitive caller changes additionally require
`make test-matrix`; new public Rustdoc examples require `cargo test --doc`.

Borrow-checker posture is currently `unknown`: this checkout has no local Rust
toolchain pin and its Cargo config contains no explicit Polonius flag. M1
records the actual compiler/wrapper flags before any ownership workaround. Use
the shared Cargo cache and existing compiler admission; never bypass the
wrappers or build in `/tmp`. Follow `AGENTS.md` if storage or admission fails.

All commands run from the repository root. During M1, use CodeGraph for
`exec_code`/tool execution callers and impact; use `firecrawl-mcp` for current
official Monty API and protocol research. Do not depend on stale RFC examples
without checking the pinned version.

For each implementation milestone, run focused tests first, then the full
repository gates sequentially, capturing each output. The repository gate
commands are:

```bash
set -o pipefail
make check-fmt 2>&1 | tee /tmp/check-fmt-axinite-2-2-1-helper-subprocess-wrapper-for-monty.out
make lint 2>&1 | tee /tmp/lint-axinite-2-2-1-helper-subprocess-wrapper-for-monty.out
make test 2>&1 | tee /tmp/test-axinite-2-2-1-helper-subprocess-wrapper-for-monty.out
```

Never run these concurrently for this worktree. Check `set -o pipefail` or the
repository shell wrapper so `tee` does not hide a failing command. Run the
project's pinned formal command as a separate check:

```bash
make verus-monty-capabilities 2>&1 | tee /tmp/verus-axinite-monty.out
```

M1 specifies the tool pins; M3 adds `make verus-monty-capabilities` and
`make install-verus` using the pinned `prover-tools` runner. The corresponding
runner operations are:

```bash
prover-tools verus install --repo-root .
prover-tools verus run --repo-root . --proof-file verus/monty_capabilities.rs
```

The Make targets must resolve the pinned runner from
`tools/rust-prover-tools/REF`, preserve its exit status, and verify download
checksums from `tools/verus/SHA256SUMS`. Proof acceptance requires a non-zero
verified-obligation count and zero errors; `FORMAL-SKIP` is not acceptance.
These are future commands, not claims that this checkout already has proofs. If
runtime dependencies change, run `make audit` after normal gates. For changed
Markdown, run `git diff --check`, `make markdownlint`, and `make spelling`; run
`make nixie` if diagrams change. Keep test feature files next to the relevant
tool tests and follow `docs/rstest-bdd-users-guide.md` for runner conventions.

## Validation and acceptance

Before coding, the focused red commands must fail because the helper,
projection, or tool behaviour is missing. Record the exact expected failing
assertion rather than accepting a compilation failure unrelated to the
contract. After the smallest implementation, rerun the focused tests (green),
then refactor with the same tests still green. At every milestone rerun
`make check-fmt`, `make lint`, and `make test` sequentially as required by the
roadmap request. M3 additionally requires the pinned Verus command. Final
documentation changes require Markdown lint, spelling, and diff checks.

The external workflow passes when `exec_code` receives this complete source,
`params={"value": 42}`, `state={}`, and `allowed_tools=[]`:

```python
async def main(params, state):
    return {"result": params["value"], "state": state}
```

The helper appends the validated entrypoint invocation
`await main(params, state)`. The result must be `{"result": 42, "state": {}}`;
requesting a tool outside that empty set returns a typed denial and leaves its
side-effect count at zero. With a permitted tool, the callback traverses caller
preflight and approval, its redacted JSON result is available to the program,
and no sentinel secret is present in logs or returned output. A helper panic
ends only that run; a subsequent invocation in the same parent succeeds.

Quality criteria:

- **Tests:** unit `rstest`, BDD `rstest-bdd`, property `proptest`, snapshot
  `insta` for stable errors, and real helper E2E all pass for happy, unhappy,
  and edge cases. Use `googletest` matchers for behavioural assertions and
  `pretty_assertions` for structured equality differences.
- **Formal verification:** the Verus proof establishes capability projection
  exactness, subset/non-widening, and empty-denies-all over arbitrary finite
  inputs from the stated premises, with no admitted obligations. Production
  uses the verified executable kernel, with a drift check for any generated
  artefact. Property tests supplement this bridge; they do not prove it.
  Process and caller integration remain outside proof scope.
- **Lint/format/test:** `make check-fmt`, `make lint`, `make test` pass after
  each major milestone. `make audit` passes if dependency metadata changes.
- **Security:** process environment and descriptors are constrained; all
  callback paths pass the parent broker; helper failure, oversized frames,
  approval denial, cancellation, and output redaction fail closed.
- **Performance:** startup and one tool round-trip are measured against the
  existing user-facing tool timeout. The provisional total wall bound is
  reviewed in M1; do not turn performance measurement into a relaxation of
  capability or approval checks.

## Idempotence and recovery

Test helper modes must be isolated from production configuration and must not
be registerable as user tools. Process cleanup is idempotent: kill-if-running,
wait/reap, close pipes, and discard protocol state on every terminal path.
Retried runs start fresh helpers and fresh in-memory snapshots. Never resume a
prior turn's Monty snapshot or replay an effectful callback automatically. When
a milestone fails, retain logs and the smallest failing test; revert only the
uncommitted milestone or use the preceding committed plateau as the recovery
point.

## Artefacts and notes

The implementation is expected to create or update, subject to M1's call-graph
findings:

- a helper package/executable with Monty linkage and JSON pipe protocol;
- parent-side process adapter, capability projection, broker and `exec_code`
  tool in the existing tools ownership area;
- focused unit, property, BDD and real-process test modules;
- `verus/monty_capabilities.rs` plus pinned install/run tooling;
- RFC/design, developer, user, roadmap, feature parity if applicable, and
  contents documentation updates.

Record exact helper launch configuration, selected Monty version, protocol
version, provisional/final limits, Verus command, gate transcripts, and any
platform-specific skips in this section as implementation proceeds.

## Interfaces and dependencies

Proposed user-facing operation:

```text
exec_code(code, allowed_tools, params=None, state=None, entrypoint="main", limits=None) -> {
  "result": <JSON value>,
  "state": <JSON object>
}
```

`allowed_tools` is mandatory and empty means no host tool access. Omitted or
null `params` and `state` normalize to empty objects. `limits` may only lower
the configured host ceilings; it cannot disable them. Enforce an object at the
top level for `params` and `state`; limit all cross-boundary values to JSON
null, booleans, numbers, strings, arrays, and objects. Fail closed on non-JSON
values, missing required result state, malformed callback data, or invalid
top-level output. Return typed errors distinguishing script error, tool
denial/failure, approval denial, resource limit, timeout, cancellation,
protocol failure, and helper crash; do not expose raw secrets or unredacted
tracebacks.

Proposed helper protocol messages are versioned request/response variants for
`start_run`, `host_call`, `resume_run`, `cancel_run`, and one terminal outcome.
Protocol review may rename the messages to match Monty's real pause/resume
surface, but may not add direct child tool execution or an unbounded message
stream.

The helper alone depends on the selected Monty Rust crate. The parent uses
existing Tokio process/pipe support and existing Axinite tool, approval,
redaction, and JSON types where their contracts fit. Check the manifest before
adding crates: `rstest`, `rstest-bdd`, `insta`, `pretty_assertions`, and
`proptest` are already present; add the authorized `googletest` dependency for
matcher assertions. Formal tooling is pinned separately as recommended in
`docs/formal-verification-methods-in-axinite.md` and must not become a runtime
dependency.

### Concrete protocol and resource decisions

Use a four-byte unsigned big-endian length followed by exactly that many UTF-8
JSON bytes. Reject zero, oversized, truncated, invalid UTF-8, unknown version,
and unknown variant frames before any dispatch. The length cap is checked
before payload allocation. Both peers reject trailing data and unexpected
message fields. Protocol version 1 has `hello`, `start_run`, `host_call`,
`resume_run`, `cancel_run`, `guest_output`, and `terminal` variants.
`guest_output` is accepted only while Running, never while a host callback is
pending or after termination; other states reject it. Include a parent-issued
`run_id` in each run message and a strictly increasing `call_id` in
callbacks/resumes. One callback may be outstanding at a time. Duplicate, stale,
cross-run, or unsolicited callbacks, and completion while a callback is
pending, discard the helper as `runtime_crash`. Compile occurs within
`start_run`; snapshots never cross this protocol in 2.2.1.

Apply 64-level JSON nesting and 256 KiB aggregate generated-stub limits in
addition to the byte caps above. Proposed Monty limits are 64 MiB guest heap,
256 stack frames, and one million allocations where the pinned API supports
that counter. The parent imposes a 512 MiB process address-space ceiling on
Linux before sending source, using an OS resource-limit adapter; the trusted
helper initially waits for the parent's start message. M1 verifies the exact
safe Rust resource-limit API and the limits required by the selected Monty
build; equivalent enforcement or changed ceilings require a recorded design
revision. Absence of reliable OS enforcement makes the feature unavailable. The
four-child ceiling bounds aggregate child allocation; it is not a claim that
host-wide exhaustion is impossible.

Start the total deadline at admission, before spawn or handshake. Use fail-fast
admission when all four permits are occupied, with no unbounded queue. Give
cleanup one second to terminate normally, then force termination and await
reaping; retain the permit and report cleanup failure until reap completes. The
supervisor is drained during parent shutdown. Check cancellation and deadline
immediately before dispatch and resume. Drain stdout and stderr concurrently;
stderr beyond the capture limit is discarded while draining continues.
Diagnostic text, source, arguments, and panic payloads are not logged raw. The
shared executor currently logs raw tool errors; the refactor to invoke resolved
tools must sanitize errors before tracing, not only in the broker after that
trace has already occurred. Guest print output uses a separately bounded data
channel/message within the JSON protocol and counts toward the cumulative
output limit.

A valid child result with invalid guest value/state is `script_error`; callback
result conversion failure is `tool_contract_error`; policy denial, approval
required, or host tool failure is `tool_error`. Explicit internal limit reports
are `resource_limit_exceeded`, parent deadline expiry is `execution_timeout`,
and malformed protocol or unexpected exit is `runtime_crash`. Cancellation and
helper-unavailable are distinct host-side outcomes. Do not infer an
out-of-memory cause merely from a signal exit.

Freeze the initial schema subset in M1: object keyword arguments, required
names, optional names retaining omission, JSON primitives/arrays/objects, and
an explicit additional-properties policy. Use a private missing-value sentinel
for omitted optionals; never substitute null. Reject invalid or reserved Python
identifiers and collisions with `main`, `params`, `state`, or generated
helpers. Reject unsupported schema constructs before launch; never weaken
validation to fit a stub. Keep finite binary64 floats and exact signed/unsigned
64-bit integers at the JSON boundary; reject other Monty numbers. Property
tests cover both limits and invalid values, escaping of hostile descriptions,
and optional omission versus explicit null.

### Ownership and invocation interfaces

The parent module owns `ExecCodeRequest`, `RunLimits`, `RunOutcome`, `RunError`,
`BrokerContext`, and the `MediatedToolInvoker` port. The port's operation
takes a selected tool identity, validated JSON arguments, and the run's
deadline/cancellation context; it returns scanned JSON or a typed denial.
Caller adapters construct this port from chat, scheduler, or worker policy.
Tools never import `ChatDelegate` or expose session locks to the helper.

Extend the typed tool invocation context to carry an optional caller-issued
broker capability, updating the `Tool` execution interface and affected
implementations/callers atomically. This is an internal API change, not a
compatibility shim. Existing ordinary tools ignore the optional capability;
`exec_code` requires it. It is not serializable or guest-constructible. Unknown
caller contexts supply none and therefore fail closed. Inject process, clock,
environment, and policy dependencies for tests; never mutate the test process
environment. Read `src/agent/CLAUDE.md` before caller changes.

Use `crates/monty-protocol/src/lib.rs` for wire types and bounded codecs,
`crates/monty-helper/src/main.rs` for child startup, and cohesive helper
modules for conversion and execution. A separate tiny
`crates/monty-capabilities` owns the production proof-compatible pure kernel;
its entries are stable numeric indices into an immutable selected-tool table,
not `Arc` values or registry I/O. The representation relation maps each index
to exactly one validated tool name and pinned identity. Prove exact projection
of those indices; test the host table construction and dispatch mapping. The
proof must not claim to verify external registry freshness or approval.

M1 selects a supported shared-source or deterministic proof-erasure build
bridge; M3 demonstrates the compiled runtime uses that kernel and rejects stale
generated artefacts. No trusted external proof body may bridge the gap. Solver
triggers and auxiliary lemmas wait until actual verification conditions exist.
The main process depends only on protocol and capability crates; only
`monty-helper` imports the interpreter. Re-use the broker and codecs for future
`run_script`, but add no saved-script API in this milestone.

### Review reconciliation and planning evidence

Two Wyvern agents inspected requirements and runtime seams. The six-expert
review produced the following design revisions on 2026-10-20:

- Pandalump required caller-issued authority, pinned tool identity, and
  fail-closed missing context; Wafflecat favoured one fresh child and the RFC's
  JSON protocol over a pool with retained interpreter state.
- Telefono required exact projection rather than subset-only proofs, a real
  production bridge, safe stub generation, and valid entrypoint examples.
  Dinolump required concrete test commands and mandatory assertion libraries.
- Buzzy Bee required process, memory, frame, queue, and cumulative-output
  bounds. Doggylump required cleanup after future cancellation, packaging
  evidence, secret-safe diagnostics, and no automatic replay of effects.

The trade-off is deliberate: single-use helpers and denying newly interactive
approvals limit throughput and convenience, while keeping run authority and
failure behaviour inspectable. Implementing interactive continuation is a
separate design decision. Verus proves only the small pure selection kernel;
real-process and caller-path tests remain mandatory.

The final architecture, contracts, and operations review found no remaining
material planning blockers after these revisions.

The remaining M1 questions concern exact upstream release/API compatibility,
resource-limit APIs, and the proof compilation bridge. They are explicit
feasibility conditions, not permission to weaken the contract. No
implementation has started. CodeGraph discovery returned no callable tools
during planning; Wyvern used targeted reads and shared code through context pack
`pk_trjpidzv`.

## Revision notes

- (2026-10-20) Initial plan draft, revised with dispatcher approval semantics,
  explicit broker authority, pinned tool identity, cancellation ownership,
  proof/runtime refinement, process resource limits, and JSON/stub boundaries.
  Implementation remains gated on explicit approval; M1 records verified
  upstream API and exact runtime paths.

Revision 2026-10-20: reconciled all six expert lenses, fixed verification
commands and examples, and made invocation, protocol, limits, and proof
representation decisions explicit. These remain proposals awaiting approval.
