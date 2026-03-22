# Formal verification methods in Axinite

## Executive summary

Axinite already does substantially more than basic unit testing. The
repository’s testing strategy explicitly uses layered validation rather than one
monolithic test command: fast local checks, deterministic Rust suites, browser
end-to-end (E2E), feature-matrix continuous integration (CI), coverage
workflows, and regression enforcement.[^1] The current root `Makefile` and
GitHub Actions workflows reinforce that split, and the repository already
carries a dedicated `fuzz/` crate with five libFuzzer
targets.[^2][^3][^4][^5][^6] That existing investment matters, because it
changes where formal methods will pay for themselves.

The highest-return additions are not “formalize everything”. They are:

1. **Stateright** for the job-lifecycle cluster in `src/agent/scheduler.rs`,
   `src/worker/job.rs`, `src/context/state.rs`,
   `src/orchestrator/job_manager.rs`, `src/orchestrator/reaper.rs`, and
   `src/orchestrator/auth.rs`, where the real bug surface comes from
   interleavings and split responsibility across background
   tasks.[^7][^8][^9][^10][^11][^12]
2. **Kani** for small, deterministic, security-critical logic in
   `src/tools/wasm/allowlist.rs`, plus a shared host/domain matcher extracted
   from the current WebAssembly (WASM) and sandbox allowlist
   implementations.[^13][^14][^15]
3. **Proptest** for configuration layering and registry-installer validation,
   especially in `src/config/mod.rs`, `src/settings.rs`, and
   `src/registry/installer.rs`, where the interesting failures live in
   combinations of partial inputs and precedence rules rather than in single
   fixed examples.[^16][^17][^18]
4. **Verus** only after Axinite extracts a tiny pure kernel that is actually
   worth proving for all executions — most plausibly the shared host/domain
   matcher, or a similarly crisp lifecycle kernel — not the current Tokio- and
   Docker-heavy orchestration code.[^19]

The best integration path in Axinite is a split one:

- Keep **Kani harnesses inside the main `ironclaw` package**, next to the
  internal helpers they verify.
- Put **Stateright models in a dedicated internal verification crate**, because
  those models should be abstractions of policy rather than thin wrappers around
  the runtime.
- Keep **Verus proofs outside Cargo** under a proof-only directory with pinned
  tools and wrapper scripts.
- Add **Proptest as an ordinary dev-dependency**, and keep those property tests
  in normal Rust test modules or integration tests.

That yields a pragmatic stack for Axinite:

- unit, integration, trace-driven, and browser E2E tests stay in place;
- fuzzing stays in place for parser and safety-layer robustness;
- **Proptest** adds structured generative coverage for layered configuration and
  hostile installer inputs;
- **Kani** adds exhaustive bounded checking for small pure security logic;
- **Stateright** adds explicit state-space exploration for job orchestration;
- **Verus** remains a later-stage tool for the few invariants worth freezing as
  proofs.

## Current state in Axinite

Today, Axinite is already a non-virtual Cargo workspace. The root manifest
contains `[workspace] members = ["."]`, and it explicitly excludes `fuzz/` from
the workspace.[^20] The root package remains `ironclaw`, and the current
`Makefile` drives `cargo nextest` test runs, feature-matrix variants, and
auxiliary WASM builds.[^20][^2] The current CI is already split by concern:
`test.yml` runs the main Rust matrix and several build-side checks,
`coverage.yml` handles coverage-specific instrumentation, and `e2e.yml` handles
scheduled and targeted browser E2E scenarios.[^3][^4][^5]

That means Axinite does **not** need a new testing culture. It already has one.
What it lacks is infrastructure for **proof-oriented** and **model-checking**
workflows.

Two existing testing investments matter especially here.

First, the repository’s written testing strategy already distinguishes between
deterministic Rust tests, browser E2E, trace-driven end-to-end tests, and
feature-gated backend tests.[^1] That is an ideal baseline for adding formal
methods selectively rather than trying to replace existing suites.

Second, the current fuzzing is both real and targeted, but it is not aimed at
the places where formal methods will buy the most. The fuzz crate currently
defines:

- `fuzz_safety_sanitizer`
- `fuzz_safety_validator`
- `fuzz_leak_detector`
- `fuzz_tool_params`
- `fuzz_config_env`[^6]

Those targets exercise the safety layer and schema validation paths: the
sanitizer, validator, leak detector, and tool-schema
validation.[^21][^22][^23][^24] Crucially, despite its name, `fuzz_config_env`
currently fuzzes the same safety-layer pipeline rather than configuration
precedence or merge semantics.[^25] That is one reason configuration layering
still looks like a strong Proptest target.

There is also a useful alignment with the current coverage plan. The current
`COVERAGE_PLAN.md` calls out `src/registry/installer.rs`,
`src/orchestrator/job_manager.rs`, `src/agent/scheduler.rs`, and
`src/config/mod.rs` among the more significant uncovered files.[^26] Coverage
alone does not prove formal methods are the right answer, but in this case the
list lines up with the places where stronger verification really would add
something beyond one more ordinary test.

## Where formal methods should go first

### First priority: the job-lifecycle cluster

The strongest **Stateright** target in Axinite is the job-lifecycle cluster:

- `src/agent/scheduler.rs`
- `src/worker/job.rs`
- `src/context/state.rs`
- `src/orchestrator/job_manager.rs`
- `src/orchestrator/reaper.rs`
- `src/orchestrator/auth.rs`[^7][^8][^9][^10][^11][^12]

Each file, taken alone, looks defensible. The risk appears when they are
composed.

`Scheduler::schedule_with_context()` deliberately holds a write lock across the
check-and-insert path to avoid time-of-check to time-of-use (TOCTOU)
double-scheduling. `Scheduler::stop()` removes the scheduled job entry, sends
`Stop`, waits briefly, aborts if the task is still running, and then attempts to
transition the context to `Cancelled`.[^7] The worker, after a successful run,
only marks a job `Completed` if the state is not already terminal, `Completed`,
or `Stuck`, and its loop stops not just on `Cancelled`, `Failed`, and
`Accepted`, but also on `Stuck`, `Completed`, and `Submitted`.[^8] Meanwhile,
`JobState::is_terminal()` only treats `Accepted`, `Failed`, and `Cancelled` as
terminal; `Completed`, `Submitted`, and `Stuck` remain active according to
`is_active()`.[^9]

On the container side, `complete_job()` stores the completion result, marks the
handle stopped, removes the Docker container, and revokes the token, but
intentionally keeps the handle in memory until the separate `cleanup_job()` call
removes it after the result has been read.[^10] The reaper, in turn, treats any
non-terminal state as active and explicitly comments that `Pending`,
`InProgress`, `Completed`, `Submitted`, and `Stuck` all prevent reaping.[^11]
The token store is also job-scoped, constant-time compared, in-memory only, and
revoked on cleanup paths.[^12]

That is textbook Stateright territory. The dominant risk is **not** arithmetic,
parser correctness, or single-function branching. It is the space of
interleavings:

- stop racing with successful completion;
- completion racing with reaper observation;
- cleanup racing with token validation;
- worker progress racing with state transitions to `Stuck`, `Submitted`, or
  `Cancelled`;
- orphan cleanup racing with retained result handles.

Use **Stateright** to model those transitions explicitly. Kani and Verus should
not be the starting point here, because this is a state-exploration problem
rather than a bounded symbolic-input problem.

### Second priority: `src/tools/wasm/allowlist.rs`

`src/tools/wasm/allowlist.rs` is the cleanest **Kani** target in the
repository.[^13]

It is small, mostly pure, and security-critical. It already encodes a crisp
policy:

- fail closed on an empty allowlist;
- require HTTPS by default;
- reject URL userinfo;
- lower-case host and scheme;
- normalize the path;
- reject invalid percent-encoding;
- reject encoded path separators;
- allow only if a pattern’s host, path, and method constraints all succeed.[^13]

That is exactly the kind of code that benefits from bounded exhaustive checking.

The current unit tests are good, but they remain hand-chosen examples. Kani adds
value by checking properties such as:

- **normalization idempotence** for small bounded paths;
- **no successful decision after invalid percent-encoding**;
- **no successful decision for any userinfo form**;
- **if a request is allowed, then some pattern’s host/path/method constraints
  really do hold**;
- **changing `allow_http()` only weakens the scheme requirement, not the rest of
  the policy**.

This should be the starting point before Kani touches any of the async
orchestration code.

### Third priority: shared host/domain matching semantics

Axinite currently has a subtle but real semantic split across the codebase.

In the WASM capability layer, `EndpointPattern::host("*.example.com")` matches
subdomains like `api.example.com`, but the tests explicitly say it does **not**
match the base domain `example.com`.[^14] In the sandbox proxy allowlist,
`DomainPattern::new("*.example.com")` does match both subdomains and the base
domain itself, and the tests explicitly assert that behaviour.[^15]

That mismatch might be deliberate. It might also be accidental drift. Either
way, it is exactly the sort of rule that should not live in two different
implementations without an explicit contract.

This is another place where stronger verification would help, but only **after a
small refactor**:

1. extract a shared pure matcher;
2. decide whether wildcard patterns include or exclude the apex domain;
3. prove or exhaustively check the shared matcher’s semantics once, rather than
   testing two divergent implementations forever.

Use **Kani** first for the extracted matcher. Consider **Verus** only if the
invariant is important enough to freeze as a proof for all executions.

### Fourth priority: configuration layering in `src/config/mod.rs` and `src/settings.rs`

Configuration layering is the strongest **Proptest** target in the current
repository.[^16][^17]

Axinite now has at least two distinct loading paths:

- `Config::from_db_with_toml()` documents the priority as **env var > TOML
  config file > database settings > default**.[^16]
- `Config::from_env_with_toml()` loads from the environment and falls back to
  legacy `settings.json`, then overlays TOML on top.[^16][^17]

On top of that, `Settings::merge_from()` only applies overlay values that differ
from `Default`, and `merge_non_default()` recursively preserves the base when
the overlay leaves a field at its default.[^17]

Those are not good Kani targets. They are good **generated property** targets.

The interesting bugs here usually involve combinations of partially populated
layers:

- base settings from the database or legacy JSON;
- partial TOML overlays;
- env vars that should override both;
- dotted-path database round-trips through `to_db_map()` / `from_db_map()`;
- defaults that must survive when an overlay leaves a field untouched.

**Proptest** should be added with properties such as:

- merging with `Settings::default()` is identity;
- a non-default TOML overlay changes only the fields it touches;
- `to_db_map()` followed by `from_db_map()` preserves all representable
  settings;
- `from_db_with_toml()` honours the documented precedence contract;
- `from_env_with_toml()` honours its documented legacy fallback behaviour;
- explicit env vars always win over the lower layers they shadow.

The repository does not currently carry `proptest` in `dev-dependencies`, so
this would be a targeted new addition rather than a refactor of an existing
property suite.[^20]

### Fifth priority: `src/registry/installer.rs`

`src/registry/installer.rs` is the other strong **Proptest** candidate.[^18]

This code validates and combines several classes of attacker-controlled input:

- artifact URLs must be HTTPS;
- hosts must be in the allowed set;
- IP literals are rejected;
- `source.dir` must stay within expected relative prefixes and avoid traversal
  components;
- `source.capabilities` must be a bare filename;
- downloaded bytes may need SHA-256 verification;
- `.tar.gz` extraction enforces a 100 MB per-entry cap and matches by filename
  while ignoring archive directory prefixes.[^18]

That combination is a classic property-testing surface. The bugs will not
usually appear in isolated examples; they appear in hostile combinations such
as:

- valid-looking URLs with surprising hosts;
- source directories that are almost but not quite safe;
- archives with duplicate names, deep prefixes, or oversized entries;
- pinned and unpinned artifact URLs with fallback behaviour;
- manifests that are partly valid and partly malicious.

**Proptest** should start here before Kani is considered for any extracted pure
helper.

### Low-priority or ordinary-testing-only modules

Early formal-method effort should not be spent on `validate_bind_mount_path()`
in `src/orchestrator/job_manager.rs`.[^10]

The file itself documents a TOCTOU gap between `canonicalize()` and the later
Docker bind mount and explicitly accepts that gap in Axinite’s current
single-tenant design. That is a **design-boundary** issue, not a good Kani or
Verus target in the current architecture.

Keep that path under ordinary tests, code review, and threat-model review unless
the multi-tenant story changes.

## Decisions Axinite should make before writing proofs

Three design questions need explicit answers before any of this becomes CI
gates.

### 1. What should `*.example.com` mean everywhere?

Right now, Axinite has at least two different answers:

- WASM `EndpointPattern` excludes the base domain;
- sandbox `DomainPattern` includes it.[^14][^15]

Choose one rule and encode it once.

The two plausible positions are:

- **Position A:** `*.example.com` matches subdomains only. The apex
  `example.com` must be listed explicitly.
- **Position B:** `*.example.com` matches both subdomains and the apex domain.

**Position A** is preferable. It is more conservative, aligns with common DNS
intuition, and reduces the chance that a policy author grants more than
intended.

### 2. Which lifecycle predicates are actually authoritative?

Axinite currently uses different lifecycle classifications for different jobs:

- `JobState::is_terminal()` is relatively narrow;
- the worker loop stops on a broader set;
- the reaper uses `is_active()` and therefore keeps `Completed`, `Submitted`,
  and `Stuck` jobs alive.[^8][^9][^11]

That might be the right design. But it is not one contract; it is several.

Choose one of these positions:

- **Position A:** introduce explicit predicates such as `should_worker_stop`,
  `is_reapable`, and `keeps_result_handle`, and stop asking a single enum helper
  to do all the semantic work.
- **Position B:** keep the current split implicit and document the special cases
  in comments.

**Position A** is preferable. A Stateright model needs explicit semantics, not a
pile of partially overlapping helper methods.

### 3. What is the post-completion retention contract?

`complete_job()` revokes the token and removes the Docker container, but it
deliberately leaves the handle and completion result in memory until
`cleanup_job()` runs after the result is read.[^10]

That is a sensible design, but the contract should be explicit:

- is result retention until explicit cleanup a guaranteed API property?
- may a background cleanup remove completed handles eagerly in future?
- should the reaper ever touch completed-but-unread results?

A model checker cannot infer the intended answer. It needs one.

## Recommended repository layout

### Preferred layout

Axinite does **not** need to be converted into a workspace, because it already
is one.[^20] The right move is to **extend** the existing workspace.

The recommended layout is:

```text
.
├── Cargo.toml
├── Makefile
├── scripts/
│   ├── install-kani.sh
│   ├── install-verus.sh
│   └── run-verus.sh
├── tools/
│   ├── kani/
│   │   └── VERSION
│   └── verus/
│       ├── VERSION
│       └── SHA256SUMS
├── crates/
│   └── axinite-verification/
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs
│       │   └── job_lifecycle_model/
│       └── tests/
│           ├── verification_harness.rs
│           └── job_lifecycle.rs
├── tests/
│   ├── property_config_layering.rs
│   └── property_registry_installer.rs
├── verus/
│   ├── axinite_proofs.rs
│   └── host_matcher.rs
└── src/
    └── tools/
        └── wasm/
            ├── allowlist.rs
            ├── shared_host_matcher.rs   # extracted
            └── kani.rs
```

That gives each technique a home that matches the way it actually works.

### Why this split is better than a single “formal” crate

This is the central structural recommendation in this document.

Use **four different homes**:

1. **Kani harnesses in the main `ironclaw` package**
   - best for internal helpers and small state machines;
   - avoids widening the public API just to satisfy proof harnesses;
   - aligns with Kani’s `cargo kani` package-oriented workflow.[^27]

2. **Stateright models in `crates/axinite-verification`**
   - best for abstract policy models and shared checker harnesses;
   - keeps model-checking code out of the shipping runtime;
   - lets the model use ordinary Rust test execution.[^28][^29]

3. **Verus proofs under `verus/` outside Cargo**
   - best for pinned binaries, separate installation, and proof-only modules;
   - matches Verus’s installation and execution model.[^19][^30]

4. **Proptest in ordinary test modules**
   - best for generated behavioural properties;
   - needs no special crate or tool runner;
   - should feel like normal Rust testing, because that is what it is.

If all of this is forced into one crate, the result will either pollute the
public API, make local execution awkward, or both.

### Root `Cargo.toml` changes

The current root manifest already has a workspace section, so the change is
small.[^20]

Add the verification crate to `members`, keep the current `exclude` list intact,
and add `proptest` as a normal dev-dependency:

```toml
[workspace]
members = [".", "crates/axinite-verification"]
# keep the current exclude list, including "fuzz"
default-members = ["."]
resolver = "3"

[dev-dependencies]
proptest = "1"
```

A note on `default-members`: in a non-virtual workspace, Cargo already defaults
root commands to the root package if `default-members` is omitted.[^31] So this
line is technically optional. It is still worth adding, because it makes the
intent explicit once the verification crate exists.

## Kani integration

### Kani tooling model

Kani is a bounded model checker for Rust. The recommended project integration
path is `cargo kani`, which runs proof harnesses against a Cargo package rather
than acting as a completely separate build system.[^27] Installation is a
two-step process: `cargo install --locked kani-verifier`, followed by
`cargo kani setup` to install the backend binaries.[^32]

That makes Kani a good fit for Axinite’s main crate, as long as the harnesses
stay focused and local.

### Why Kani belongs in the main Axinite package

Axinite’s best Kani targets sit close to internal helpers:

- URL parsing and path normalization in `src/tools/wasm/allowlist.rs`;
- a small extracted host/domain matcher shared by the WASM and sandbox
  allowlists.[^13][^14][^15]

Moving those harnesses into an external crate would either lose access to
non-public helpers or start exporting internals only for the verifier. That is a
poor trade.

Instead, add module-adjacent harnesses under `#[cfg(kani)]`, for example:

```rust
// src/tools/wasm/mod.rs
#[cfg(kani)]
mod kani;
```

Use integration-style Kani harnesses only for APIs that are already public.

### Recommended Kani targets

#### Phase 1 smoke harnesses

These should run on every pull request.

1. **userinfo rejection**
   - any URL with `user@host` or `user:pass@host` must be denied, regardless of
     the allowlist host.[^13]

2. **path normalization safety**
   - invalid percent-encoding is always rejected;
   - encoded path separators are always rejected;
   - normalization is idempotent on bounded inputs.[^13]

3. **empty allowlist denial**
   - no request may be allowed when the allowlist is empty.[^13]

4. **allowed implies real predicate satisfaction**
   - if validation returns `Allowed`, then some pattern’s host, path, and method
     constraints truly hold.[^13][^14]

5. **shared host matcher semantics**
   - once extracted, the matcher must follow the chosen wildcard contract and
     never accept unrelated suffix tricks such as
     `example.com.evil.org`.[^14][^15]

#### Phase 2 full harnesses

These can run nightly or by manual dispatch:

- larger bounded path shapes;
- more host/method combinations;
- equivalence checks between the shared matcher and both call sites;
- more aggressive normalization edge cases.

### Smoke versus full targets

Use a split target structure:

```make
kani: ## Run practical Kani smoke harnesses
	cargo kani -p ironclaw --harness verify_allowlist_userinfo_rejected
	cargo kani -p ironclaw --harness verify_allowlist_normalize_path_small
	cargo kani -p ironclaw --harness verify_shared_host_matcher_small

kani-full: ## Run all Kani harnesses
	cargo kani -p ironclaw
```

That keeps the PR path practical while still allowing deeper checking elsewhere.

### Unwinding policy

Kani supports both explicit per-harness unwind annotations and command-line
unwind controls.[^33]

Use this rule in Axinite:

- prefer **`#[kani::unwind(N)]`** when the bound is semantically meaningful;
- use `--default-unwind` only as a coarse fallback for smoke suites.

That keeps the bound close to the reason it exists and reduces the chance of
mistaking an arbitrary global bound for a justified proof boundary.

### Optional later step: Kani contracts

Kani also has contract support.[^34] Do **not** start there.

Consider contracts later only if:

- a shared pure helper ends up called by many harnesses;
- Kani runtimes become large enough that verified stubbing would help;
- the extracted shared matcher grows into a reusable contract boundary.

### A representative Kani harness shape

This is the sort of harness Axinite should write first:

```rust
#[cfg(kani)]
mod kani {
    use crate::tools::wasm::allowlist::AllowlistValidator;
    use crate::tools::wasm::capabilities::EndpointPattern;

    #[kani::proof]
    fn verify_userinfo_never_allows_request() {
        let tail = if kani::any() {
            "api.openai.com"
        } else {
            "evil.com"
        };

        let url = format!("https://user:pass@{tail}/v1/chat/completions");

        let validator = AllowlistValidator::new(vec![
            EndpointPattern::host("api.openai.com").with_path_prefix("/v1/"),
        ]);

        assert!(!validator.validate(&url, "GET").is_allowed());
    }
}
```

That is small, stable, and checks a load-bearing security property.

## Stateright integration

### Stateright tooling model

Stateright is a Rust model checker for nondeterministic systems. Models
implement a `Model` trait, are supplemented with `always` and `sometimes`
properties, and are explored by a checker obtained from `Model::checker()`,
typically via `spawn_bfs()` (breadth-first search, BFS) or `spawn_dfs()`
(depth-first search, DFS).[^28][^29][^35]

That makes it a good fit for Axinite’s orchestration semantics, where the
dominant risk is interleaving rather than input parsing.

### Why Stateright belongs in a separate verification crate

The job-lifecycle model should be an **abstraction of policy**, not a rerun of
Tokio internals. That is precisely what an internal verification crate is for.

A good starting `Cargo.toml` is:

```toml
[package]
name = "axinite-verification"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
stateright = "0.30"
ironclaw = { path = "../..", default-features = false }

[dev-dependencies]
rstest = "0.26"
```

If the first model can stay completely proof-only, an even lighter option is to
omit the `ironclaw` dependency and re-declare the tiny abstract state needed.
The point of the crate is isolation, not reuse for its own sake.

### Model scope

Model the job-lifecycle semantics, not the concrete runtime implementation.

The model state should represent things like:

- whether a job exists;
- the job’s logical `JobState`;
- whether a scheduler entry exists;
- whether a worker is idle, running, stopping, or gone;
- whether a container handle exists, and if so whether it is creating, running,
  or stopped;
- whether a completion result is present;
- whether the token is absent, live, or revoked;
- whether the reaper sees the job as active or orphaned;
- whether explicit cleanup has occurred.

The actions should be nondeterministic lifecycle events such as:

- `Dispatch`
- `Schedule`
- `WorkerStart`
- `WorkerOk`
- `WorkerFail`
- `WorkerTimeout`
- `StopRequested`
- `CompleteJob`
- `CleanupJob`
- `ReaperScan`
- `ReaperReap`
- `ValidateToken`
- `RecoverFromStuck`
- `SubmitForReview`
- `Accept`

That is intentionally more abstract than the runtime. That is a feature, not a
bug.

### Properties to encode

#### Safety (`Property::always`)

1. **No duplicate live execution for one job**
   - a single logical job never has two live scheduler/worker/container
     executions at once.

2. **Token scope is preserved**
   - a token may validate only for its own job;
   - once revoked, it never validates again without a fresh create
     operation.[^12]

3. **Result retention is coherent**
   - a completion result can exist only after completion;
   - cleanup removes retained handle state, not before.[^10]

4. **Reaper does not reap active work**
   - if the model says a job is still active by the chosen lifecycle contract,
     the reaper must not remove its container.[^11][^9]

5. **Stop/complete races do not produce impossible combinations**
   - for example, no state where a token is revoked, the container is still
     running, and the system still believes normal worker execution is active.

6. **Monotone post-cleanup absence**
   - once explicit cleanup succeeds, no handle or token remains.

#### Reachability (`Property::sometimes`)

1. a job can complete successfully;
2. a job can be cancelled;
3. a stuck job can either recover or fail;
4. an orphaned container can be reaped;
5. a completion result can be observed before cleanup;
6. a stop request can race with completion.

The value of the model is not just in finding bad traces. It is also in proving
that the system can still reach the good ones under realistic races.

### Shared checker harness

Follow a shared harness pattern:

- inspect the model’s properties;
- split them into safety versus reachability;
- run the checker with bounded depth and state count;
- stop early once the desired reachability properties have examples;
- fail if any safety property has a discovery or any required reachability
  property does not.[^28][^29][^35]

A reasonable starting budget for Axinite is:

- `target_max_depth(8)`
- `target_state_count(10_000)`
- `spawn_bfs()` on pull requests

Then add a deeper nightly run later.

### Suggested Stateright file layout

```text
crates/axinite-verification/
├── src/
│   ├── lib.rs
│   └── job_lifecycle_model/
│       ├── mod.rs
│       ├── model.rs
│       ├── state.rs
│       ├── action.rs
│       └── properties.rs
└── tests/
    ├── verification_harness.rs
    └── job_lifecycle.rs
```

### Why not use Stateright’s actor API first?

Stateright does have an actor framework and `ActorModel`.[^36] That may become
useful later if Axinite wants to model multiple interacting jobs or higher-level
service behaviour.

For the first Axinite model, Axinite should still implement `Model` directly,
because:

- the first target is a single logical job lifecycle;
- the risk surface is policy and interleaving, not network transport;
- a direct model gives tighter control over the abstraction boundary.

## Verus integration

### Tooling model

Verus is a tool for verifying the correctness of Rust code against explicit
specifications, with a focus on full functional correctness of low-level systems
code.[^19] Its installation model is separate from Cargo: the upstream guidance
is to download a release, unzip it, run `./verus`, and install the Rust
toolchain it requests.[^30]

That means Axinite should treat Verus as a separate proof runner, not as “just
another cargo test”.

### Why Verus should _not_ live inside the main build

Do not try to make Verus look like an ordinary Cargo test target.

Use:

- `tools/verus/VERSION`
- `tools/verus/SHA256SUMS`
- `scripts/install-verus.sh`
- `scripts/run-verus.sh`
- `make verus`

That gives the repository:

- a pinned binary;
- reproducible local and CI installs;
- no pollution of the normal Rust toolchain;
- room for proof-only files under `verus/`.

### What Verus should prove in Axinite

Start narrow. Very narrow.

The first worthwhile Verus target is **not** the scheduler or the Docker
orchestration. It is a proof-only model of a **shared host/domain matcher**,
after Axinite has extracted one and chosen its wildcard contract.

#### Best first proof obligations

1. **Exact hosts behave exactly**
   - an exact host pattern matches if and only if the normalized host is
     identical.

2. **Wildcard hosts follow the chosen contract**
   - if the rule is “subdomains only”, the apex never matches;
   - if the rule is “include apex”, that behaviour is explicit and proved.

3. **Suffix spoofing is impossible**
   - `example.com.evil.org` and similar superstrings never match
     `*.example.com`.

4. **Normalization is semantics-preserving**
   - lowercasing and any chosen bracket stripping do not create or destroy
     matches unexpectedly.

5. **Delegating both allowlist call sites to the shared kernel removes semantic
   drift**
   - once both modules call the same proof-backed matcher, they cannot diverge
     silently again.

Those are crisp invariants. They are the kind Verus can justify.

### What Verus should not prove first

Do **not** begin with:

- `src/agent/scheduler.rs`
- `src/worker/job.rs`
- `src/orchestrator/job_manager.rs`
- Docker integration
- `reqwest` / URL parser internals
- the whole configuration loader
- the whole registry installer

That path would consume time and produce little near-term value.

### Proof style recommendation

Keep the first Verus files as **proof-only modules with proof-specific types**
rather than trying to verify the production structs verbatim.

In other words:

- create a tiny `SpecHostPattern`;
- prove lemmas about exact-match and wildcard-match semantics;
- only then decide whether the production matcher should be restructured to
  align more closely with the proof model.

That is a more honest phase-1 plan than pretending the shipping implementation
will be mechanically verified end to end.

### Trigger discipline

Once Verus lands, treat trigger warnings as real engineering feedback, not as
noise to suppress. The proof code should be understandable and reviewable, not
merely accepted by the tool.

### Representative proof tree

```text
verus/
├── axinite_proofs.rs
├── host_matcher.rs
└── lifecycle_kernel.rs      # optional later
```

`axinite_proofs.rs` can simply `mod` the proof files and provide a single entry
point for `make verus`.

## Recommended Makefile changes

Axinite’s current top-level `Makefile` has no formal-verification targets.[^2]
The repository should add these:

```make
.PHONY: test-verification stateright proptest-properties kani kani-full verus formal-pr formal-nightly formal

test-verification: ## Run the Stateright verification crate
	cargo test -p axinite-verification

stateright: test-verification ## Alias for verification models

proptest-properties: ## Run targeted generated property tests
	cargo test --test property_config_layering
	cargo test --test property_registry_installer

kani: ## Run Kani smoke harnesses
	cargo kani -p ironclaw --harness verify_allowlist_userinfo_rejected
	cargo kani -p ironclaw --harness verify_allowlist_normalize_path_small
	cargo kani -p ironclaw --harness verify_shared_host_matcher_small

kani-full: ## Run all Kani harnesses
	cargo kani -p ironclaw

verus: ## Run Verus proofs
	VERUS_BIN="$(VERUS_BIN)" scripts/run-verus.sh

formal-pr: test-verification proptest-properties kani ## Fast PR gate

formal-nightly: test-verification proptest-properties kani-full verus ## Deeper scheduled gate

formal: formal-pr ## Default formal suite
```

Two notes matter here.

First, this layout uses ordinary `cargo test` for the Stateright crate rather
than `cargo nextest run`. That keeps the first model-checking harness simple and
predictable. If it later behaves cleanly under Nextest, switching the target is
trivial.

Second, Verus stays **out** of `formal-pr`. That is deliberate. On Axinite’s
current priorities, Verus should arrive only after there is something small and
stable enough to prove. Kani and Stateright are earlier wins.

## Recommended CI changes

### Keep the current test, coverage, and E2E workflows intact

Axinite already has a sensible workflow split:

- `test.yml` for the Rust matrix and build-side checks;
- `coverage.yml` for coverage instrumentation;
- `e2e.yml` for scheduled and targeted browser E2E.[^3][^4][^5]

Do **not** fold formal verification into coverage generation. It should be
independently runnable, independently cacheable, and independently diagnosable.

### Add one new workflow: `formal.yml`

A separate `formal.yml` should be added rather than bloating `test.yml`.

#### 1. `stateright-models`

Run on every pull request.

```yaml
stateright-models:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v6
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - name: Run Stateright verification crate
      run: make test-verification
```

#### 2. `kani-smoke`

Run on every pull request.

```yaml
kani-smoke:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v6
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - name: Install Kani
      run: scripts/install-kani.sh
    - name: Run Kani smoke harnesses
      run: make kani
```

Where `scripts/install-kani.sh` does roughly this:

```bash
#!/usr/bin/env bash
set -euo pipefail
KANI_VERSION="$(cat tools/kani/VERSION)"
cargo install --locked kani-verifier --version "${KANI_VERSION}"
cargo kani setup
```

That keeps local and CI execution aligned with the tool’s documented
installation path.[^32]

#### 3. `verus-proofs`

Run nightly or by manual dispatch **initially**, not on every pull request.

```yaml
verus-proofs:
  if:
    github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v6
    - name: Install Verus
      run: scripts/install-verus.sh
      env:
        VERUS_INSTALL_DIR: ${{ runner.temp }}/verus
    - name: Run Verus proofs
      run: make verus
      env:
        VERUS_BIN: ${{ runner.temp }}/verus/verus/verus
```

### Where the new Proptest suites should run

Because Proptest is still ordinary Rust testing, it should not get a separate
workflow unless it becomes materially expensive. Let the new property tests run
under `test.yml` by default, and reserve `formal.yml` for the tools that truly
need dedicated runners.

## How Proptest, fuzzing, and current tests fit after these changes

This document is about formal methods, but the right integration strategy is
additive rather than replacement-oriented.

### Add Proptest selectively

Axinite does **not** currently depend on `proptest`.[^20] Add it, but do so
selectively:

- configuration precedence and layering;
- database map round-trips;
- hostile manifest and archive validation for the registry installer.

The right relationship is:

- **Kani** for exhaustive bounded checking over small, extracted kernels;
- **Proptest** for broad generated coverage over larger structured input spaces.

Those tools complement each other.

### Keep fuzzing

The current fuzz targets remain valuable, especially for the safety layer and
tool-schema validation.[^6][^21][^22][^23][^24]

The right relationship is:

- **fuzzing** for weird parser and input-shape robustness;
- **Kani** for proofs over small pure helpers;
- **Stateright** for orchestration interleavings that fuzzing will almost never
  explore systematically.

Again, complementary rather than competing.

### Keep Axinite’s deterministic test layers

Do not weaken the current deterministic layers to “make room” for formal
methods. Axinite’s testing strategy already leans on deterministic Rust tests,
trace-driven agent tests, and browser E2E with mock upstreams.[^1]

That is exactly the right substrate for targeted formal work.

## Concrete first tasks

A practical implementation order for Axinite is:

### Phase 1: infrastructure only

1. add `proptest` to root `dev-dependencies`;
2. add `crates/axinite-verification`;
3. add `tools/kani/VERSION`;
4. add `tools/verus/*` and `scripts/install-verus.sh` / `scripts/run-verus.sh`;
5. add `make test-verification`, `make proptest-properties`, `make kani`,
   `make kani-full`, and `make verus`;
6. add `formal.yml`.

### Phase 2: low-risk, high-return verification

1. add Kani harnesses for `src/tools/wasm/allowlist.rs`;
2. add Proptest suites for `src/config/mod.rs` and `src/settings.rs`;
3. add Proptest suites for `src/registry/installer.rs`.

### Phase 3: actor modelling

1. create `JobLifecycleModel` in `crates/axinite-verification`;
2. add safety and reachability properties;
3. gate a bounded breadth-first search (BFS) checker in pull request (PR) CI;
4. extend the model to include token revocation and retained completion results.

### Phase 4: proof-worthy invariants

1. decide wildcard host semantics;
2. extract a shared host/domain matcher;
3. add Kani equivalence harnesses for both call sites;
4. if the matcher still looks proof-worthy after extraction, add a small Verus
   proof.

That ordering is deliberate. It pushes the highest bug-finding return to the
front and delays the most proof-maintenance cost until Axinite has settled the
relevant contracts.

## What to avoid

A few antipatterns are worth ruling out explicitly.

### Do not verify the Tokio/Docker orchestration with Verus first

That is the wrong tool for the dominant risk surface.

### Do not put Kani, Stateright, Verus, and Proptest all in one crate

Each technique wants a different relationship with production code.

### Do not widen the public API just to satisfy proofs

If Kani needs crate-internal access, keep the harness inside the main package.

### Do not assume `fuzz_config_env` already covers configuration precedence

Despite the name, that target currently fuzzes the safety-layer pipeline rather
than the configuration loader.[^25]

### Do not let `JobState::is_terminal()` stand in for every lifecycle predicate

The worker, reaper, and container manager clearly need more than one semantic
classification.[^8][^9][^11][^10]

### Do not treat the current bind-mount TOCTOU gap as a proof problem

Under the current single-tenant design, that is a design trade-off, not a proof
target.[^10]

### Do not default formal jobs to `--all-features`

Axinite’s normal matrix already exercises feature permutations.[^3][^4] Formal
jobs should compile the **smallest feature set that still exercises the target
invariant**.

## Final recommendation

The most coherent Axinite plan is:

- **add Proptest selectively** for configuration layering and registry-installer
  semantics;
- **add Kani harnesses inside the main package** for
  `src/tools/wasm/allowlist.rs` and the extracted shared host matcher;
- **add `crates/axinite-verification`** for a Stateright model of
  scheduler/worker/container/reaper/token lifecycle;
- **treat Verus as an optional later-stage tool** for a tiny pure kernel, not as
  part of the initial rollout.

If Axinite does only the first tranche of this work, make it:

1. verification infrastructure and a separate `formal.yml` workflow;
2. Kani on the WASM allowlist;
3. Proptest on configuration layering and the registry installer;
4. Stateright on the job lifecycle.

That would give Axinite the highest bug-finding return for the least
organizational friction.

## References

[^1]:
    Axinite testing strategy:
    <https://github.com/leynos/axinite/blob/main/docs/testing-strategy.md>

[^2]: Axinite `Makefile`: <https://github.com/leynos/axinite/blob/main/Makefile>

[^3]:
    Axinite test workflow:
    <https://github.com/leynos/axinite/blob/main/.github/workflows/test.yml>

[^4]:
    Axinite coverage workflow:
    <https://github.com/leynos/axinite/blob/main/.github/workflows/coverage.yml>

[^5]:
    Axinite E2E workflow:
    <https://github.com/leynos/axinite/blob/main/.github/workflows/e2e.yml>

[^6]:
    Axinite fuzz crate manifest:
    <https://github.com/leynos/axinite/blob/main/fuzz/Cargo.toml>

[^7]:
    Axinite `src/agent/scheduler.rs`:
    <https://github.com/leynos/axinite/blob/main/src/agent/scheduler.rs>

[^8]:
    Axinite `src/worker/job.rs`:
    <https://github.com/leynos/axinite/blob/main/src/worker/job.rs>

[^9]:
    Axinite `src/context/state.rs`:
    <https://github.com/leynos/axinite/blob/main/src/context/state.rs>

[^10]:
    Axinite `src/orchestrator/job_manager.rs`:
    <https://github.com/leynos/axinite/blob/main/src/orchestrator/job_manager.rs>

[^11]:
    Axinite `src/orchestrator/reaper.rs`:
    <https://github.com/leynos/axinite/blob/main/src/orchestrator/reaper.rs>

[^12]:
    Axinite `src/orchestrator/auth.rs`:
    <https://github.com/leynos/axinite/blob/main/src/orchestrator/auth.rs>

[^13]:
    Axinite `src/tools/wasm/allowlist.rs`:
    <https://github.com/leynos/axinite/blob/main/src/tools/wasm/allowlist.rs>

[^14]:
    Axinite `src/tools/wasm/capabilities.rs`:
    <https://github.com/leynos/axinite/blob/main/src/tools/wasm/capabilities.rs>

[^15]:
    Axinite `src/sandbox/proxy/allowlist.rs`:
    <https://github.com/leynos/axinite/blob/main/src/sandbox/proxy/allowlist.rs>

[^16]:
    Axinite `src/config/mod.rs`:
    <https://github.com/leynos/axinite/blob/main/src/config/mod.rs>

[^17]:
    Axinite `src/settings.rs`:
    <https://github.com/leynos/axinite/blob/main/src/settings.rs>

[^18]:
    Axinite `src/registry/installer.rs`:
    <https://github.com/leynos/axinite/blob/main/src/registry/installer.rs>

[^19]: Verus guide: <https://verus-lang.github.io/verus/guide/>

[^20]:
    Axinite `Cargo.toml`:
    <https://github.com/leynos/axinite/blob/main/Cargo.toml>

[^21]:
    Axinite `fuzz_safety_sanitizer`:
    <https://github.com/leynos/axinite/blob/main/fuzz/fuzz_targets/fuzz_safety_sanitizer.rs>

[^22]:
    Axinite `fuzz_safety_validator`:
    <https://github.com/leynos/axinite/blob/main/fuzz/fuzz_targets/fuzz_safety_validator.rs>

[^23]:
    Axinite `fuzz_leak_detector`:
    <https://github.com/leynos/axinite/blob/main/fuzz/fuzz_targets/fuzz_leak_detector.rs>

[^24]:
    Axinite `fuzz_tool_params`:
    <https://github.com/leynos/axinite/blob/main/fuzz/fuzz_targets/fuzz_tool_params.rs>

[^25]:
    Axinite `fuzz_config_env`:
    <https://github.com/leynos/axinite/blob/main/fuzz/fuzz_targets/fuzz_config_env.rs>

[^26]:
    Axinite `COVERAGE_PLAN.md`:
    <https://github.com/leynos/axinite/blob/main/COVERAGE_PLAN.md>

[^27]: Kani usage guide: <https://model-checking.github.io/kani/usage.html>

[^28]:
    Stateright crate documentation:
    <https://docs.rs/stateright/latest/stateright/>

[^29]:
    Stateright `CheckerBuilder` documentation:
    <https://docs.rs/stateright/latest/stateright/struct.CheckerBuilder.html>

[^30]:
    Verus installation instructions:
    <https://github.com/verus-lang/verus/blob/main/INSTALL.md>

[^31]:
    Cargo workspace reference:
    <https://doc.rust-lang.org/cargo/reference/workspaces.html>

[^32]:
    Kani install guide:
    <https://model-checking.github.io/kani/install-guide.html>

[^33]:
    Kani unwind attribute reference:
    <https://model-checking.github.io/kani/reference/attributes.html#kaniunwindnumber>

[^34]:
    Kani contracts reference:
    <https://model-checking.github.io/kani/reference/experimental/contracts.html>

[^35]:
    Stateright `Property` documentation:
    <https://docs.rs/stateright/latest/stateright/struct.Property.html>

[^36]:
    Stateright actor module documentation:
    <https://docs.rs/stateright/latest/stateright/actor/index.html>
