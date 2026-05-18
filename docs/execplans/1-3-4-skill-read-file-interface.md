# Add the `skill_read_file` interface for bundled skill resources

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: DRAFT

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
`tests/channels/skills_upload.rs`, and
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
- If adding Python `pytest-bdd` for end-to-end behaviour coverage requires a
  new dependency in `tests/e2e/pyproject.toml`, keep that dependency scoped to
  the e2e package and document why. Do not add Python tooling to the Rust
  runtime.
- Use `rstest` fixtures for Rust unit and integration tests. Use `pytest` for
  Python end-to-end tests. Add `pytest-bdd` scenarios for user-requested
  behavioural e2e coverage unless a concrete repository conflict is found; if
  so, stop and record the conflict.
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
  If `pytest-bdd` creates dependency or CI friction, record the exact failure
  and ask whether to keep Python BDD or substitute the existing Rust
  `rstest-bdd` layer.
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

- Risk: Python `pytest-bdd` is not currently part of the e2e test package.
  Severity: medium.
  Likelihood: high.
  Mitigation: add it only to `tests/e2e/pyproject.toml` for behaviour coverage,
  keep the scenario focused, and preserve the existing `pytest` suite style.
  Escalate if the project maintainers prefer Rust `rstest-bdd` only.

- Risk: response-size caps may conflict with existing `SKILL.md` loading caps.
  Severity: low.
  Likelihood: medium.
  Mitigation: define a named read cap in the skill read policy, keep it at or
  below the existing prompt-oriented cap unless RFC review dictates otherwise,
  and snapshot the error payload for oversized reads.

- Risk: e2e tests may need a deterministic model tool-call flow that the
  current mock LLM does not yet provide.
  Severity: medium.
  Likelihood: medium.
  Mitigation: prefer a direct HTTP/API e2e test if the server exposes tool
  execution enough for it; otherwise extend `tests/e2e/mock_llm.py` with the
  smallest deterministic tool-call fixture.

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
- [ ] Receive explicit approval for this ExecPlan before implementation.
- [ ] Implement domain path policy and typed response model.
- [ ] Implement and register `skill_read_file`.
- [ ] Add unit, property, behavioural, snapshot, and e2e coverage.
- [ ] Update documentation and mark roadmap item `1.3.4` done.
- [ ] Run `coderabbit review --agent` after each major implementation
  milestone and resolve concerns.
- [ ] Run final gates and commit the approved implementation.

## Surprises & discoveries

- Observation: `leta workspace add` succeeded, but the first Rust LSP query
  failed because `rust-analyzer` was not installed for the active toolchain.
  Evidence: `leta grep ...` reported that the Rust language server failed to
  start and suggested `rustup component add rust-analyzer`.
  Impact: install the component and restart the `leta` daemon before relying
  on semantic Rust navigation.

- Observation: `pytest` is used for Python/Playwright e2e tests, but
  `pytest-bdd` is not currently in `tests/e2e/pyproject.toml`.
  Evidence: the e2e package lists `pytest`, `pytest-asyncio`,
  `pytest-playwright`, `pytest-timeout`, `playwright`, `aiohttp`, and `httpx`,
  while Rust-side BDD is already present through `rstest-bdd`.
  Impact: the approved implementation should either add a small e2e
  `pytest-bdd` scenario because the task explicitly requests it, or stop if
  that dependency conflicts with repository policy.

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
  Rationale: the repository already uses `rstest-bdd` for Rust behavioural
  tests, while the user explicitly requested `pytest` and `pytest-bdd`.
  Keeping `pytest-bdd` scoped to the e2e package honours the request without
  changing the Rust runtime test stack.
  Date/Author: 2026-05-19 / Codex.

- Decision: Use `proptest` for path policy and decline Kani/Verus unless a
  stronger proof obligation emerges during implementation.
  Rationale: the new invariant is bounded input validation over path strings
  and file classifications. Property tests are proportionate and already used
  in nearby skill and dispatcher tests; Kani or Verus would be disproportionate
  unless the design introduces a formal business axiom or unsafe code.
  Date/Author: 2026-05-19 / Codex.

## Outcomes & retrospective

This plan is not yet implemented. The expected outcome after approval is a
single read-only `skill_read_file` tool that reads only loaded skill bundle
resources through stable bundle-relative paths, plus tests and documentation
that prove the feature works without exposing raw filesystem access.

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
canonical skill name. Add Python e2e coverage under `tests/e2e/` using
`pytest`; add `pytest-bdd` only to that e2e package and create a small
Given/When/Then scenario for installing or loading a bundled skill, invoking
`skill_read_file`, and observing a structured text success plus a structured
denial. Use the mock LLM or direct HTTP/tool invocation path, whichever is the
smallest deterministic system-level route.

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
cd /home/leynos/.lody/repos/github---leynos---axinite/worktrees/b0245e42-5cad-47f8-8090-c3eb643834a6
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
- Python e2e coverage with `pytest` and, if added, `pytest-bdd` proves the
  externally observable read flow or records a documented blocker.
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

If Python e2e dependency setup fails because `pytest-bdd` is not accepted by
the repository, preserve the Rust `rstest-bdd` behavioural coverage, document
the conflict in this plan, and ask for approval before dropping the Python BDD
requirement.

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

## Artifacts and notes

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
