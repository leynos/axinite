# Add the `skill_read_file` interface for bundled skill resources

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

Roadmap item `1.3.4` completes the runtime access path for multi-file skill
bundles. Roadmap items `1.3.1`, `1.3.2`, and `1.3.3` already validate passive
`.skill` archives, preserve `SKILL.md`, `references/`, and `assets/` during
installation, and store the canonical runtime root plus bundle-relative
entrypoint in each `LoadedSkill`. The missing behaviour is a model-callable,
read-only interface that can read a specific bundle-relative file without
giving the model raw host filesystem access.

After this change, a model can call `skill_read_file` with a canonical skill
identifier such as `deploy-docs` and a path such as `references/usage.md`.
Text files under `SKILL.md`, `references/**`, and text-compatible
`assets/**` are returned inline through a stable JSON payload. Absolute paths,
path traversal, unsupported locations, unknown skills, binary assets, and
oversized files return deterministic skill-scoped error payloads. This lets
skills use progressive disclosure: activate the skill, read `SKILL.md`, and
then lazily read only the referenced bundled resources that are needed.

The implementation must not start until this plan is explicitly approved.

## Approval gates

The first gate is plan approval. A human reviewer must explicitly approve this
ExecPlan before implementation starts. Silence is not approval.

The second gate is implementation completion. The implementer must finish the
domain path policy, tool adapter, tool registration, attenuation update,
tests, documentation, and roadmap update without widening generic filesystem
access.

The third gate is milestone review. After each major milestone, run
`coderabbit review --agent`, resolve or record every concern, and continue only
when the review has no unresolved blocking findings.

The fourth gate is validation. Run the targeted tests while developing and the
final repository gates before committing: `make check-fmt`, `make lint`,
`make test`, and the aggregate `make all` if the individual target transcript
does not already satisfy the repository gate. Long-running validation commands
must be run sequentially through `tee` to log files under `/tmp`.

The fifth gate is documentation sync. Update user-facing and maintainer-facing
documents before marking roadmap item `1.3.4` done.

## Repository orientation

Start with `AGENTS.md`, `docs/contents.md`,
`docs/welcome-to-axinite.md`, and
`docs/axinite-architecture-overview.md`. These describe the repository rules,
product direction, and top-level runtime shape. `docs/roadmap.md` defines
item `1.3.4` as the read-only bundled-resource access task and states the
success rule: the model can read bundle-relative files, oversized or
disallowed files fail through a skill-scoped error path, and raw filesystem
access is not exposed.

`docs/rfcs/0003-skill-bundle-installation.md` is the design authority for this
feature. Keep the sections `Problem`, `Reference Model`, `Runtime Model
Interface`, `Why A Dedicated Tool Instead Of Raw File Paths`, `Security
Considerations`, `Testing`, and `Rollout Plan` open while implementing. The
RFC suggests the model-facing schema, successful response, typed non-inline
error response, and tool semantics for `skill_read_file`.

`docs/execplans/1-3-2-extend-skill-installation-flows-bundles.md` and
`docs/execplans/1-3-3-persist-canonical-skill-roots-in-the-loaded-model.md`
record the completed prerequisites. The important inherited invariant is that
archive and install policy lives in `src/skills/`, while web handlers and
tool adapters only translate transport input into those shared policies.

`src/skills/mod.rs` contains the core runtime model. `LoadedSkillLocation`
stores a private filesystem root, a bundle-relative entrypoint, a canonical
skill identifier, and `SkillPackageKind`. `LoadedSkill::skill_root()`,
`LoadedSkill::skill_entrypoint()`, and `LoadedSkill::package_kind()` are the
expected accessors for resolving reads.

`src/skills/bundle/mod.rs` and `src/skills/bundle/path.rs` already own archive
validation for the bundle layout: `SKILL.md`, `references/**`, and `assets/**`
are allowed; `scripts/`, traversal, links, duplicate normalized paths, and
executables are rejected during installation. Do not duplicate archive
validation, but reuse the same vocabulary for runtime read policy.

`src/skills/registry.rs` exposes `SkillRegistry::skills()`. The new reader can
look up an installed or loaded skill by `manifest.name` and resolve paths
against that loaded skill's private root. If this lookup shape becomes awkward,
add the smallest registry helper in `src/skills/registry.rs` rather than
letting the tool adapter scan private fields or infer install paths itself.

`src/tools/builtin/skill_tools.rs`, `src/tools/builtin/skill_tools/install.rs`,
and `src/tools/builtin/skill_tools/remove.rs` show the existing
registry-backed skill tools. `src/tools/registry/builtins.rs` registers skill
management tools with the active `ToolRegistry`. `src/tools/builtin/mod.rs`
re-exports built-in tool types. The new tool should follow these local
patterns.

`src/skills/attenuation.rs` contains `READ_ONLY_TOOLS`, the conservative list
of tools available when installed skills lower the visible tool ceiling.
`skill_read_file` belongs there only after the implementation is provably
read-only, skill-scoped, non-networked, and non-mutating.

`src/tools/builtin/file.rs` and `src/tools/builtin/path_utils.rs` contain
generic file-tool behaviour and path helpers. Treat these as implementation
references, not as a capability to expose. `skill_read_file` must not call the
generic `read_file` tool or return absolute paths to the model.

The relevant test bases are `src/skills/bundle/tests.rs`,
`src/skills/registry/tests/install.rs`,
`src/skills/registry/tests/discovery.rs`,
`src/skills/registry/tests/prop_tests.rs`,
`src/tools/builtin/skill_tools/tests.rs`,
`src/agent/dispatcher/tests/skill_bundle_context_bdd.rs`,
`tests/channels/skills_upload.rs`, and end-to-end (e2e)
`tests/e2e/scenarios/test_skills.py`.

The implementation should load these skills before editing:

- `leta`, for symbol navigation and reference checks.
- `rust-router`, then the smallest relevant Rust follow-on skill. For this
  work, expect `rust-types-and-apis` for schema and value types,
  `rust-errors` for response/error shape, and `rust-memory-and-state` only if
  registry sharing or locks become unclear.
- `hexagonal-architecture`, as boundary discipline: domain policy belongs in
  `src/skills/`; built-in tools and web/e2e paths are adapters.
- `domain-web-services`, only if the implementation adds or changes web
  gateway behaviour.
- `nextest`, if debugging or filtering `cargo nextest` runs becomes
  necessary.
- `commit-message` and `pr-creation`, when committing or updating the pull
  request.

External prior art supports the same narrow shape. Model Context Protocol
(MCP) resources describe file-like context as resources read by URI, with
explicit URI validation and permission checks before reads. OpenAI Apps SDK tool
annotations include `readOnlyHint` for tools that retrieve information without
modifying external state. Axinite does not need to adopt either protocol
wholesale, but these references support an explicit read-only descriptor,
structured input/output schema, and validation before resource access.[^1][^2]

## Constraints

- Do not implement raw local filesystem reads, directory listing, globbing, or
  arbitrary path access. `skill_read_file` may only resolve against a loaded
  skill's canonical private root.
- Do not return absolute host paths in successful or error responses. The
  model-facing response may include only the canonical `skill` and
  bundle-relative `path`.
- Allow only `SKILL.md`, `references/**`, and `assets/**` as
  bundle-relative paths. Reject absolute paths, empty paths, parent directory
  traversal, Windows-style drive prefixes, repeated root prefixes, and paths
  outside those allowed locations.
- Keep archive validation and install policy in `src/skills/`. The tool
  adapter must call shared policy or a domain helper rather than reimplementing
  independent rules in `src/tools/`.
- Return text inline only. Phase 1 must not return base64 content or binary
  bytes for assets.
- Return deterministic JSON error payloads for unknown skills, unreadable
  paths, binary assets, oversized files, invalid UTF-8, and I/O failures. The
  native `ToolError` should be reserved for malformed tool parameters or
  unexpected adapter failures where no model-facing typed response can be
  produced.
- Preserve existing single-file `SKILL.md` installs and active-skill prompt
  injection behaviour.
- Avoid new Rust runtime dependencies. `mime_guess`, `serde`, `serde_json`,
  `thiserror`, `tokio`, `rstest`, `rstest-bdd`, `insta`, and `proptest` are
  already available.
- Use `rstest` fixtures for Rust unit and integration tests. Use
  `rstest-bdd` for behavioural Rust coverage where the behaviour is best
  expressed as a scenario. Use the existing Python `pytest` e2e style only
  when the change affects externally observable server workflows.
- Use `proptest` for path-policy invariants over a range of path inputs.
  Do not add Kani or Verus unless the implementation introduces a stronger
  invariant that cannot be covered credibly by property tests and review.
- Do not mark roadmap item `1.3.4` done until implementation, tests,
  documentation, `coderabbit review --agent`, and final gates pass.

If satisfying the objective requires violating a constraint, stop, document the
conflict in `Decision Log`, and ask for direction.

## Tolerances

- Scope: if implementation needs changes to more than 12 non-test Rust source
  files or more than 700 net non-documentation lines, stop and reassess the
  boundary design.
- Interface: if the work requires changing public HTTP routes, database
  schema, extension WIT contracts, or the generic `read_file` contract, stop
  and ask for approval.
- Tool schema: adding one model-callable built-in tool named
  `skill_read_file` is in scope. Adding additional tool methods, directory
  listings, partial reads, or binary fetch URLs is out of scope unless
  explicitly approved.
- Dependencies: if any new Rust runtime dependency appears necessary, stop.
  New test-only dependencies require a short decision-log entry explaining why
  existing `rstest`, `rstest-bdd`, `proptest`, or `pytest` coverage is
  insufficient.
- Security: if a proposed design exposes absolute paths, follows symlinks out
  of the skill root, reads through a link target, or falls back to generic
  filesystem tools, stop immediately.
- Tests: if targeted tests still fail after three focused fix attempts, stop
  and document the failing command, log path, and failure summary.
- Validation time: if any single gate approaches the 1200-second command
  limit, stop the current command, capture the log, and split the next run into
  smaller documented pieces.
- Ambiguity: if multiple valid interpretations of "assets text files" affect
  output shape or allowed media types, choose the conservative interpretation:
  UTF-8 text may be returned inline, binary or oversized content returns the
  typed non-inline error.

## Risks

- Risk: the current `ToolError` type is flat and string-oriented, while RFC
  0003 requires structured skill-scoped errors.
  Severity: medium.
  Likelihood: high.
  Mitigation: return RFC-shaped JSON as a successful `ToolOutput` for expected
  domain denials, and use `ToolError::InvalidParameters` only for malformed
  tool-call parameters.

- Risk: symlinks or time-of-check/time-of-use races could escape the skill
  root even though archive validation rejects symlinks.
  Severity: high.
  Likelihood: medium.
  Mitigation: validate the lexical bundle path before joining, canonicalize the
  final target where practical, reject symlink metadata, and verify the
  resolved file remains below the loaded skill root.

- Risk: adding `skill_read_file` to `READ_ONLY_TOOLS` before the behaviour is
  fully constrained would weaken installed-skill attenuation.
  Severity: high.
  Likelihood: low.
  Mitigation: add attenuation only after unit tests prove no writes, network
  calls, generic filesystem paths, or state mutation are involved.

- Risk: Python e2e coverage may need deterministic tool-call plumbing rather
  than a direct unit-level invocation.
  Severity: medium.
  Likelihood: medium.
  Mitigation: prefer the existing Python `pytest` e2e style if this becomes
  externally observable. If the existing mock model or HTTP route cannot drive
  a deterministic tool call, record the blocker and keep the Rust
  `rstest-bdd` behavioural coverage as the primary scenario layer.

- Risk: response-size caps may conflict with existing `SKILL.md` loading caps.
  Severity: low.
  Likelihood: medium.
  Mitigation: define a named read cap in the skill read policy, keep it at or
  below the existing prompt-oriented cap unless RFC review dictates otherwise,
  and snapshot the error payload for oversized reads.

## Progress

- [x] (2026-05-19 00:00+02:00) Loaded `leta`, `rust-router`,
  `hexagonal-architecture`, `execplans`, `firecrawl-mcp`,
  `en-gb-oxendict-style`, `commit-message`, and `pr-creation` skills relevant
  to this planning task.
- [x] (2026-05-19 00:00+02:00) Created a `leta` workspace for this worktree.
- [x] (2026-05-19 00:01+02:00) Confirmed the starting branch was
  `feat/skill-read-file-plan`, not the main branch, then renamed it to
  `1-3-4-skill-read-file-interface`.
- [x] (2026-05-19 00:02+02:00) Used a Wyvern agent team for read-only
  reconnaissance across implementation seams, roadmap/RFC requirements, and
  testing strategy.
- [x] (2026-05-19 00:06+02:00) Used Firecrawl to check external prior art for
  MCP resources and OpenAI read-only tool annotations.
- [x] (2026-05-19 00:10+02:00) Drafted this pre-implementation ExecPlan.
- [x] (2026-05-19 00:22+02:00) Ran `coderabbit review --agent`, applied the
  wording and punctuation findings, and reran it to zero findings.
- [x] (2026-05-20 12:00+02:00) Reopened the planning context on the existing
  `1-3-4-skill-read-file-interface` branch, confirmed it tracks
  `origin/1-3-4-skill-read-file-interface`, and found the earlier draft PR
  `#187` closed.
- [x] (2026-05-20 12:00+02:00) Refreshed external prior-art checks with
  Firecrawl against the MCP resources specification and OpenAI Apps SDK
  reference.
- [x] (2026-05-20 12:00+02:00) Used a Wyvern agent team for a fresh read-only
  planning brief and incorporated its boundary and testing cautions.
- [x] (2026-05-20 12:00+02:00) Corrected stale plan assumptions about Python
  `pytest-bdd`; the implementation should use Rust `rstest-bdd` for
  behavioural coverage and existing Python `pytest` e2e patterns only where
  system-level behaviour changes.
- [x] (2026-05-20 21:46+02:00) Received explicit user instruction to proceed
  with implementation under this ExecPlan.
- [x] (2026-05-20 21:46+02:00) Confirmed the working branch is
  `1-3-4-skill-read-file-interface`, clean, and tracking
  `origin/1-3-4-skill-read-file-interface`.
- [x] (2026-05-20 21:57+02:00) Implemented the first Rust slice:
  domain-owned `src/skills/file_read.rs`, `SkillReadFileTool`, built-in tool
  registration, attenuation allow-listing, `rstest` unit coverage,
  `proptest` path-policy coverage, and `rstest-bdd` behavioural scenarios.
- [x] (2026-05-20 21:57+02:00) Ran targeted validation:
  `cargo test --features test-helpers skills::file_read -- --nocapture`
  passed 11 tests; `cargo test --features test-helpers
  skill_read_file_tool -- --nocapture` passed 2 tests; `cargo test --features
  test-helpers skill_read_file_schema -- --nocapture` passed 1 test; and
  `cargo test --features test-helpers bdd_model -- --nocapture` passed 2
  `rstest-bdd` scenario tests.
- [x] Implement domain path policy and typed response model.
- [x] Implement and register `skill_read_file`.
- [x] Add unit, property, and behavioural coverage.
- [x] Update documentation and mark roadmap item `1.3.4` done.
- [x] Run `coderabbit review --agent` after each major implementation
  milestone and resolve concerns.
- [x] Run final gates.
- [ ] Commit the approved implementation.

## Surprises & discoveries

- Observation: `leta workspace add` succeeded, but the first Rust Language
  Server Protocol (LSP) query failed because `rust-analyzer` was not installed
  for the active toolchain.
  Evidence: `leta grep ...` reported that the Rust language server failed to
  start and suggested `rustup component add rust-analyzer`.
  Impact: install the component and restart the `leta` daemon before relying
  on semantic Rust navigation.

- Observation: after resuming implementation on 2026-05-20, `leta workspace
  add` reported the workspace was already present, but `leta show` and
  `leta refs` failed with `EOF while parsing a value at line 1 column 0`.
  Evidence: direct calls for `LoadedSkillLocation`,
  `register_skill_tools`, and `READ_ONLY_TOOLS` all failed with the same
  parser error.
  Impact: continue with direct source inspection and `rg` for this milestone,
  and keep the failure documented so navigation assumptions are not hidden.

- Observation: `pytest` is used for Python/Playwright e2e tests, but
  `pytest-bdd` is not currently in `tests/e2e/pyproject.toml`.
  Evidence: the e2e package lists `pytest`, `pytest-asyncio`,
  `pytest-playwright`, `pytest-timeout`, `playwright`, `aiohttp`, and `httpx`,
  while Rust-side BDD is already present through `rstest-bdd`.
  Impact: do not add Python `pytest-bdd` for this roadmap item unless a later
  approved design specifically needs it. Use Rust `rstest-bdd` for
  behavioural coverage and the existing Python `pytest` style for any
  necessary e2e checks.

- Observation: RFC 0003 asks for stable structured error payloads, but the
  native tool error enum is not shaped for domain-denial JSON.
  Evidence: `src/tools/tool/traits.rs` and `src/error/tool.rs` expose variants
  such as `InvalidParameters` and `ExecutionFailed`, while existing tools
  return structured successful `ToolOutput` values for normal results.
  Impact: expected skill-scoped denials should be JSON tool results, not
  generic execution failures.

- Observation: `mime_guess` is already a normal dependency.
  Evidence: `Cargo.toml` contains `mime_guess = "2.0.5"`.
  Impact: media type inference for non-inline asset metadata does not require
  a new Rust dependency.

- Observation: the initial filtered test command
  `cargo test --features test-helpers skill_read_file -- --nocapture` matched
  no tests because the new tests live under module and scenario names rather
  than a single shared test name.
  Evidence: the command passed while reporting zero executed tests; the later
  module-specific commands executed the intended domain, adapter, schema, and
  BDD tests.
  Impact: use explicit module filters such as `skills::file_read`,
  `skill_read_file_tool`, `skill_read_file_schema`, and `bdd_model` when
  rerunning the targeted suite.

- Observation: CodeRabbit caught four useful implementation concerns during
  the first milestone review: incomplete Rustdoc, excessive function
  complexity, an over-large inline test module, and a size-cast comment that
  needed to match the bounded comparison.
  Evidence: `/tmp/coderabbit-skill-read-file-slice-axinite-1-3-4-skill-read-file-interface.out`.
  Impact: split policy I/O and validation into submodules, moved tests into
  `src/skills/file_read/tests.rs`, completed public docs, and simplified the
  size conversion explanation before continuing.

- Observation: a second CodeRabbit pass identified a replacement race between
  lexical validation and file reads.
  Evidence:
  `/tmp/coderabbit-skill-read-file-slice-rerun-axinite-1-3-4-skill-read-file-interface.out`.
  Impact: revalidate the opened file metadata after opening so symlinks,
  non-regular files, and oversized files still fail through the
  skill-scoped error path even if a bundle file changes between checks.

- Observation: the CodeRabbit review after the split reported one valid
  cleanup finding: an opened file descriptor's metadata cannot report the
  path as a symlink, so the post-open symlink check was dead code.
  Evidence:
  `/tmp/coderabbit-skill-read-file-docs-final-axinite-1-3-4-skill-read-file-interface.out`.
  Impact: remove the ineffective `is_symlink()` branch while retaining the
  pre-open symlink rejection, post-open regular-file check, and size
  revalidation.

- Observation: the final CodeRabbit pass asked for explicit symlink
  regression coverage.
  Evidence:
  `/tmp/coderabbit-skill-read-file-clean-final-axinite-1-3-4-skill-read-file-interface.out`.
  Impact: add a Unix unit test that creates a bundle-relative symlink under
  `references/` and verifies `read_skill_file` returns `path_not_readable`.

- Observation: the follow-up CodeRabbit pass identified missing positive path
  policy property coverage, bare-directory cases, unclear PNG fixture bytes,
  and a remaining symlink time-of-check/time-of-use gap.
  Evidence:
  `/tmp/coderabbit-skill-read-file-final-clear-axinite-1-3-4-skill-read-file-interface.out`.
  Impact: add positive generated cases for `references/**` and `assets/**`,
  assert `SKILL.md` validates, cover bare `references/` and `assets/`, use a
  real PNG signature for the binary fixture, and open files with
  `O_NOFOLLOW` on Unix before post-open metadata validation.

- Observation: the next CodeRabbit retry hit a recoverable rate limit.
  Evidence:
  `/tmp/coderabbit-skill-read-file-final-after-fixes-axinite-1-3-4-skill-read-file-interface.out`
  reported `rate_limit` with a suggested wait time of 55 seconds.
  Impact: continue with local gates and retry CodeRabbit before committing.

- Observation: the post-gate CodeRabbit review asked for a clearer audit
  trail on the non-Unix fallback, which cannot use Unix `O_NOFOLLOW`.
  Evidence:
  `/tmp/coderabbit-skill-read-file-after-gates-axinite-1-3-4-skill-read-file-interface.out`.
  Impact: document that non-Unix currently relies on the earlier
  `symlink_metadata` rejection plus the later opened-file size revalidation,
  and leave a TODO for platform-specific atomic no-follow semantics.

- Observation: the immediate CodeRabbit retry after the non-Unix fallback
  comment hit another recoverable rate limit.
  Evidence:
  `/tmp/coderabbit-skill-read-file-final-reviewed-axinite-1-3-4-skill-read-file-interface.out`
  reported `rate_limit` with a suggested wait time of 2 minutes and
  28 seconds.
  Impact: wait and retry before committing so the latest CodeRabbit concern
  has a clean follow-up result if the service quota permits it.

- Observation: the waited CodeRabbit retry still hit a recoverable rate
  limit, with a longer suggested delay.
  Evidence:
  `/tmp/coderabbit-skill-read-file-final-retry-axinite-1-3-4-skill-read-file-interface.out`
  reported `rate_limit` with a suggested wait time of 5 minutes and
  34 seconds.
  Impact: treat CodeRabbit availability as the only remaining external review
  constraint; local gates continue to run, and one more retry will be made
  before commit.

- Observation: a final CodeRabbit retry after the clean aggregate gate still
  hit the same service-side rate limit.
  Evidence:
  `/tmp/coderabbit-skill-read-file-final-last-axinite-1-3-4-skill-read-file-interface.out`
  reported `rate_limit` with a suggested wait time of 5 minutes and
  36 seconds.
  Impact: all previously reported CodeRabbit concerns are fixed, but the
  final clean-review confirmation is blocked by CodeRabbit service quota. Do
  not treat this as a code concern; include it in the handoff and PR context.

- Observation: final local validation passed after the last source and
  ExecPlan changes.
  Evidence: `/tmp/markdownlint-final-axinite-1-3-4-skill-read-file-interface.out`
  reported zero Markdown errors; `/tmp/diff-check-final-axinite-1-3-4-skill-read-file-interface.out`
  was clean; `/tmp/all-final-axinite-1-3-4-skill-read-file-interface.out`
  passed `make all`, including 4091 nextest tests and 5 GitHub tool tests.
  Impact: the implementation is locally gated and ready to commit.

- Observation: the 2026-05-25 review correctly identified that canonicalizing
  a target and then reopening it by path was still a time-of-check/time-of-use
  gap, including for intermediate symlink components.
  Evidence: `src/skills/file_read/io.rs` previously stored a canonical target
  path in `CanonicalTarget` and later opened that path in
  `read_file_contents`.
  Impact: replace the path reopen with a Linux `openat2` call anchored to the
  canonical skill-root directory file descriptor and using
  `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`, then read from that opened handle.
  Non-Linux targets now fail closed with a skill-scoped I/O error rather than
  using a weaker plain `File::open` fallback.

- Observation: the same review requested exact maximum-size coverage and
  clearer documentation/comment spelling.
  Evidence: review comments called out the missing
  `MAX_SKILL_READ_FILE_BYTES` boundary case, two Rustdoc comments using
  non-Oxford spelling, the stale skill tool registry summary, and first-use
  definitions for `e2e` and `LSP`.
  Impact: add an exact-size success test, update the Rustdoc and registry
  comments, and define end-to-end (e2e) and Language Server Protocol (LSP) in
  the ExecPlan.

- Observation: no Python e2e test was added for this slice.
  Evidence: the implementation changes a model-facing built-in tool contract
  but does not add a new HTTP route, CLI command, persistence workflow, UI
  flow, or network boundary. Rust `rstest-bdd` scenarios exercise the
  externally visible tool contract through the built-in tool adapter.
  Impact: keep system-level coverage focused on the Rust tool contract and
  avoid adding a Python e2e path that would duplicate lower-level assertions
  without covering a distinct server workflow.

- Observation: the requested branch already existed locally and remotely.
  Evidence: `git branch --list --verbose --verbose
  '*1-3-4-skill-read-file-interface*'` showed a local branch tracking
  `origin/1-3-4-skill-read-file-interface`, and `gh pr view` showed the
  associated pull request `#187` was closed.
  Impact: continue on the existing tracking branch, update the plan in place,
  and create a new draft pull request after the refreshed plan is committed and
  pushed.

## Decision log

- Decision: Write this as a pre-implementation plan only and do not implement
  `skill_read_file` until approval.
  Rationale: the `execplans` skill requires an explicit approval gate, and the
  user specifically stated that the plan must be approved before
  implementation.
  Date/Author: 2026-05-19 / Codex.

- Decision: Treat `skill_read_file` as a built-in tool adapter backed by a
  domain-owned path policy in `src/skills/`.
  Rationale: hexagonal architecture applies here as boundary discipline. The
  skill subsystem owns bundle roots and path rules; the built-in tool is only
  the driving adapter that accepts JSON and returns JSON.
  Date/Author: 2026-05-19 / Codex.

- Decision: Use structured JSON `ToolOutput` for expected policy denials such
  as `path_not_readable`, `non_inline_asset`, and `file_too_large`.
  Rationale: RFC 0003 defines model-facing error payloads, and using native
  `ToolError` for normal denials would collapse useful error codes into
  generic execution failures.
  Date/Author: 2026-05-19 / Codex.

- Decision: Plan both Rust `rstest-bdd` coverage and Python `pytest-bdd` e2e
  coverage.
  Rationale: this decision was superseded on 2026-05-20 after re-reading the
  task. The request names `rstest-bdd`, not Python `pytest-bdd`, and the
  repository already has Python e2e tests in plain `pytest`.
  Date/Author: 2026-05-19 / Codex. Superseded 2026-05-20 / Codex.

- Decision: Use Rust `rstest-bdd` for behavioural coverage and keep Python e2e
  tests in the existing `pytest` style when system-level coverage is needed.
  Rationale: this matches the user request and avoids adding a second BDD
  framework to the Python e2e package without a concrete need.
  Date/Author: 2026-05-20 / Codex.

- Decision: Use `proptest` for path policy and decline Kani/Verus unless a
  stronger proof obligation emerges during implementation.
  Rationale: the new invariant is bounded input validation over path strings
  and file classifications. Property tests are proportionate and already used
  in nearby skill and dispatcher tests; Kani or Verus would be disproportionate
  unless the design introduces a formal business axiom or unsafe code.
  Date/Author: 2026-05-19 / Codex.

- Decision: begin implementation after the 2026-05-20 user approval message
  and keep this ExecPlan as the live delivery log.
  Rationale: the approval gate is satisfied by the explicit request to
  proceed with implementation of this plan.
  Date/Author: 2026-05-20 / Codex.

- Decision: do not add snapshot tests for this change.
  Rationale: the stable model-facing JSON shape is asserted directly in unit
  and behavioural tests, including success, policy-denial, and unknown-skill
  responses. Snapshot files would add review churn without making the
  contract more precise for this small response vocabulary.
  Date/Author: 2026-05-20 / Codex.

- Decision: mark roadmap item `1.3.4` done as part of the implementation
  branch, before the final commit.
  Rationale: the code, user documentation, internal documentation, and feature
  parity notes now describe the implemented behaviour; final gates remain as
  the commit condition rather than a separate roadmap semantics change.
  Date/Author: 2026-05-20 / Codex.

## Outcomes & retrospective

Implementation is complete and awaiting final repository gates and commit.
The branch now contains a single read-only `skill_read_file` tool that reads
only loaded skill bundle resources through stable bundle-relative paths, plus
tests and documentation proving the feature works without exposing raw
filesystem access.

## Plan of work

Stage A defines the domain policy. Add a small policy module under
`src/skills/`, for example `src/skills/file_read.rs`, and export it from
`src/skills/mod.rs` or a narrow submodule path. The module should define the
request and response vocabulary used by the built-in tool: a validated
bundle-relative path type, success response, expected error response,
non-inline metadata, and a semantic error enum. It should expose a function
with a shape close to:

```rust
pub async fn read_skill_file(
    skill: &LoadedSkill,
    requested_path: &str,
) -> SkillReadFileResponse;
```

The exact signature may change if the implementation requires injection of
filesystem operations for tests, but the policy must accept a `LoadedSkill` or
equivalent domain object rather than a raw root path from the model.
Validation in this stage should cover allowed paths, rejected paths, unknown
or unsupported path forms, file size limits, binary detection, invalid UTF-8,
and metadata for non-inline responses. Use `proptest` for the lexical path
invariant: generated
absolute paths, traversal segments, alternate separators, and repeated root
prefixes must never produce a resolved path outside the skill root.

Stage B adds the tool adapter. Add `SkillReadFileTool` alongside the existing
skill tools, either in `src/tools/builtin/skill_tools/read_file.rs` or a
dedicated `src/tools/builtin/skill_read_file.rs` if that keeps the file under
the repository's size guidance. The tool schema should match RFC 0003:
`skill` and `path` are required strings, and `additionalProperties` is false.
The tool should acquire the registry read lock, find the loaded skill by
`manifest.name`, call the domain policy, and return the structured JSON result
through `ToolOutput::success`. Unknown skills should return a deterministic
skill-scoped JSON error rather than leaking the registry layout.

Stage C registers the tool and tightens attenuation. Export the new tool from
`src/tools/builtin/mod.rs`, register it in
`ToolRegistry::register_skill_tools()` in
`src/tools/registry/builtins.rs`, and update the log message and any registry
tests that assert tool counts. Add `skill_read_file` to
`src/skills/attenuation.rs::READ_ONLY_TOOLS` after tests prove the tool reads
only bundled skill files and remains scoped. If hosted or worker catalogue
tests assert built-in tool
schemas, update those expectations so hosted workers see the same explicit
tool contract.

Stage D adds behavioural and end-to-end coverage. Extend Rust tests with
`rstest` fixtures and a focused `rstest-bdd` feature that proves an active
skill can reference a bundled file and the model-facing tool reads it by
canonical skill name. Add Python e2e coverage under `tests/e2e/` using the
existing `pytest` style only if the change affects an externally observable
server workflow. Use the mock LLM or direct HTTP/tool invocation path,
whichever is the smallest deterministic system-level route.

Stage E updates documentation. Update `docs/users-guide.md` with the new user
behaviour, including request and response examples and phase-1 limitations.
Update `docs/agent-skills-support.md` with the internal lifecycle and
maintainer-facing boundary rules. Update `docs/developers-guide.md` with any
new conventions for skill-file policy or e2e BDD tests. Update
`docs/axinite-architecture-overview.md` if the skills row needs to mention the
new read path. Check `FEATURE_PARITY.md` for feature status changes. Mark
`docs/roadmap.md` item `1.3.4` done only after implementation and validation
are complete.

Stage F performs final review, validation, and commit. Run
`coderabbit review --agent`, resolve concerns, run the final gates through
`tee`, inspect logs, run Markdown lint for changed Markdown files, run
`git diff --check`, and commit with a file-based commit message.

## Concrete steps

All commands run from the repository root:

```bash
cd /home/leynos/.lody/repos/github---leynos---axinite/worktrees/6c92fc0f-e404-4a10-ace1-30a36066de50
```

Before editing, confirm the branch and workspace:

```bash
git branch --show-current
leta workspace add "$PWD"
```

Expected branch output:

```plaintext
1-3-4-skill-read-file-interface
```

Use `leta` for symbol navigation before editing Rust:

```bash
leta show LoadedSkillLocation
leta show LoadedSkill.skill_root
leta show 'src/tools/registry/builtins.rs:register_skill_tools'
leta refs READ_ONLY_TOOLS
```

Add red tests for Stage A and Stage B first. The initial targeted test command
should fail before implementation:

```bash
ACTION=skill-read-file-red
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
cargo test --features test-helpers skill_read_file -- --nocapture 2>&1 | tee "$LOG"
```

After implementing the domain policy and tool adapter, rerun targeted tests:

```bash
ACTION=skill-read-file-targeted
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
cargo test --features test-helpers skill_read_file -- --nocapture 2>&1 | tee "$LOG"
```

Run the Rust behavioural tests:

```bash
ACTION=skill-read-file-bdd
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
cargo test --features test-helpers skill_bundle -- --nocapture 2>&1 | tee "$LOG"
```

Run Python e2e coverage from the repository root after installing the e2e
package in the active Python environment, if necessary:

```bash
ACTION=skill-read-file-e2e
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
pytest tests/e2e/scenarios/test_skills.py -v --timeout=120 2>&1 | tee "$LOG"
```

Run `coderabbit review --agent` after major milestones:

```bash
ACTION=coderabbit-skill-read-file
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
coderabbit review --agent 2>&1 | tee "$LOG"
```

Run final gates sequentially:

```bash
ACTION=check-fmt
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make check-fmt 2>&1 | tee "$LOG"

ACTION=lint
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make lint 2>&1 | tee "$LOG"

ACTION=test
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make test 2>&1 | tee "$LOG"
```

If repository policy requires the aggregate target in addition to the explicit
subtargets, run:

```bash
ACTION=all
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make all 2>&1 | tee "$LOG"
```

Run Markdown and whitespace checks after documentation edits:

```bash
bunx markdownlint-cli2 docs/execplans/1-3-4-skill-read-file-interface.md \
  docs/users-guide.md docs/agent-skills-support.md docs/developers-guide.md \
  docs/axinite-architecture-overview.md docs/roadmap.md
git diff --check
```

Commit only after the gates pass, using the file-based commit-message workflow
from the `commit-message` skill.

## Validation and acceptance

The feature is accepted when all of the following are true:

- Calling `skill_read_file` with a loaded skill name and `SKILL.md` returns a
  JSON object containing the same `skill`, the normalized bundle-relative
  `path`, a text `mime_type`, and inline `content`.
- Calling `skill_read_file` with `references/usage.md` from an installed
  bundle returns the file content without exposing an absolute path.
- Calling `skill_read_file` with absolute paths, `..`, paths outside
  `SKILL.md`, `references/**`, or `assets/**`, unknown skill names, and missing
  files returns deterministic skill-scoped JSON errors.
- Calling `skill_read_file` for binary assets returns `error.code` equal to
  `non_inline_asset` with `metadata.size`, `metadata.mime_type`, and a stable
  `metadata.fetch_hint`.
- Calling `skill_read_file` for oversized text returns `error.code` equal to
  `file_too_large` with the same metadata fields.
- Installed-skill attenuation allows `skill_read_file` while still blocking
  generic `read_file`.
- Snapshot tests cover any stable model-facing output whose formatting matters.
- Property tests prove path validation cannot resolve outside the loaded skill
  root over generated path inputs.
- Python e2e coverage with the existing `pytest` harness proves the externally
  observable read flow or records a documented blocker, if the final
  implementation affects a system-level workflow.
- Documentation explains current user-visible and maintainer-visible
  behaviour.
- `coderabbit review --agent` has no unresolved blocking concerns.
- `make check-fmt`, `make lint`, and `make test` succeed. If `make all` is run
  as the aggregate gate, it succeeds too.

Quality criteria:

- Tests: targeted Rust tests, Rust behavioural tests, property tests, and
  relevant Python e2e tests pass.
- Lint/typecheck: clippy and formatting gates pass through the Makefile
  targets.
- Security: no raw filesystem path is exposed to the model, no generic file
  read is reused as the model-facing capability, and symlink/path traversal
  escapes are rejected.
- Documentation: changed Markdown passes markdownlint and `git diff --check`.

## Idempotence and recovery

The implementation steps are additive and can be repeated. If a targeted test
fails, inspect the corresponding `/tmp/*-axinite-1-3-4-skill-read-file-interface.out`
log before rerunning. If a staged design begins to duplicate path policy in the
tool adapter, stop and move that logic back into `src/skills/` before
continuing.

If `coderabbit review --agent` reports concerns, fix them in the smallest
logical commit or record why the concern is not applicable in `Decision Log`.
Do not proceed to the next milestone with unresolved blocking concerns.

If Python e2e setup cannot drive the new tool deterministically, preserve the
Rust `rstest-bdd` behavioural coverage, document the blocker in this plan, and
ask for approval before dropping system-level e2e coverage.

If the implementation has to be rolled back before commit, use ordinary Git
diff review and reverse patches for the files changed by this task. Do not use
`git reset --hard` or checkout commands that would discard unrelated user
work.

## Interfaces and dependencies

The model-facing tool name is:

```plaintext
skill_read_file
```

The input schema is:

```json
{
  "type": "object",
  "properties": {
    "skill": {
      "type": "string",
      "description": "Installed skill name exactly as advertised to the model."
    },
    "path": {
      "type": "string",
      "description": "Bundle-relative path, such as SKILL.md or references/usage.md."
    }
  },
  "required": ["skill", "path"],
  "additionalProperties": false
}
```

A successful text response is:

```json
{
  "skill": "deploy-docs",
  "path": "references/usage.md",
  "mime_type": "text/markdown",
  "content": "# Usage\n..."
}
```

An expected denial response is:

```json
{
  "skill": "deploy-docs",
  "path": "assets/logo.png",
  "error": {
    "code": "non_inline_asset",
    "message": "Phase 1 does not return binary or oversized assets inline.",
    "metadata": {
      "size": 18231,
      "mime_type": "image/png",
      "fetch_hint": "Treat this as a passive asset; request only referenced text files in phase 1."
    }
  }
}
```

Expected error codes are:

- `unknown_skill`
- `path_not_readable`
- `non_inline_asset`
- `file_too_large`
- `invalid_utf8`
- `io_error`

The final code should expose these concepts through named Rust types rather
than open-coded JSON fragments wherever practical. The adapter may serialize
the final response with `serde_json::json!` only at the boundary.

## Artefacts and notes

Firecrawl research used during planning:

- MCP resources are described as context-bearing data such as files or
  application-specific information. The read operation is URI-based, and the
  specification calls out URI validation, permission checks, and proper
  handling of binary data.[^1]
- OpenAI Apps SDK documentation identifies `readOnlyHint` as the tool
  annotation for tools that retrieve information without modifying external
  data and points implementers towards explicit input and output schemas.[^2]

Wyvern agent-team findings used during planning:

- The implementation seam is `LoadedSkillLocation` plus
  `ToolRegistry::register_skill_tools()`, with a new built-in tool and an
  update to `READ_ONLY_TOOLS`.
- Existing tests already cover bundle validation, install lifecycle, active
  skill context snapshots, `rstest-bdd`, and path-property testing.
- Python e2e tests use `pytest`; Python `pytest-bdd` is not yet installed,
  while Rust `rstest-bdd` is already present.

[^1]: Model Context Protocol, "Resources",
    <https://modelcontextprotocol.io/specification/2025-06-18/server/resources>.
[^2]: OpenAI Developers, "Reference - Apps SDK",
    <https://developers.openai.com/apps-sdk/reference>.

## Revision note

2026-05-19: Created the initial draft for approval. The draft captures the
planned domain boundary, tool schema, validation strategy, documentation sync,
quality gates, and approval requirement for roadmap item `1.3.4`.

2026-05-19: Applied CodeRabbit wording and punctuation findings and reran
`coderabbit review --agent` to zero findings. This does not change the planned
implementation approach.

2026-05-20: Refreshed the draft after discovering the requested branch already
existed and its previous draft pull request was closed. Corrected the concrete
worktree path and narrowed behavioural-test guidance to Rust `rstest-bdd` plus
existing Python `pytest` e2e patterns where applicable.

2026-05-25: Applied follow-up review fixes for the file-read hardening path.
The implementation now opens the bundle root as a stable directory handle
before calling Linux `openat2`, caps inline reads at
`MAX_SKILL_READ_FILE_BYTES + 1`, and makes Linux-only behavioural tests
explicit while non-Linux allowed reads fail closed with `io_error`.

2026-05-25: Verified follow-up overall comments. The per-read canonicalization
comment was stale because `open_validated_target` no longer canonicalizes the
root. The non-Linux diagnostic comment remained valid, so the fail-closed
branch now emits a warning before returning `io_error`.
