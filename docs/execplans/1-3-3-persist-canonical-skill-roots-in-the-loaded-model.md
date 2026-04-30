# Persist canonical skill roots in the loaded skill model

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: DRAFT

## Purpose / big picture

Roadmap item `1.3.3` makes multi-file skill bundles useful at runtime without
yet adding a general file-read surface. Roadmap items `1.3.1` and `1.3.2`
already validate and install passive `.skill` archives while preserving
`SKILL.md`, `references/`, and `assets/` on disk. The current loaded skill model
still behaves like a prompt-only single-file model: `LoadedSkill` records the
manifest, prompt body, trust, source, content hash, and selection caches, but it
does not retain the canonical installed root or the bundle-relative
`SKILL.md` entrypoint that later runtime reads will need.

After this change, runtime state must record the installed skill root and the
bundle-relative `SKILL.md` entrypoint for every loaded skill. Active-skill
injection must include stable identity fields so the model can see a canonical
skill identifier and bundle-relative layout in the selected skill block. The
future `skill_read_file` tool in roadmap item `1.3.4` can then resolve
`references/...` and `assets/...` against the recorded root instead of relying
on raw local filesystem paths.

Success is observable in four ways. First, loading a nested installed bundle
produces a `LoadedSkill` whose runtime metadata points at the final installed
skill directory and `SKILL.md`. Second, loading an existing flat single-file
skill still works and records a compatible entrypoint without changing trust or
selection behaviour. Third, an active skill is injected as a `<skill ...>` block
that includes escaped stable metadata such as the canonical skill identifier,
bundle root, and entrypoint while still injecting only the `SKILL.md` body.
Fourth, tests prove that install, discovery, reload, selection, prompt
injection, and trust attenuation continue to work with this extra metadata.

The plan uses `hexagonal-architecture` as boundary discipline, not as a pattern
transplant. The domain policy is the skill identity model: a loaded skill has a
canonical identifier, an on-disk root owned by the registry, an entrypoint
relative to that root, and a flag describing whether it came from a bundled
tree or a single-file install. Filesystem discovery and staged installation are
adapters that populate those values. The dispatcher is a driving adapter that
renders selected skill metadata into prompt context.

Implementation must not begin until this plan is explicitly approved.

## Approval gates

The first gate is plan approval. A human reviewer must explicitly approve this
ExecPlan before implementation starts. Silence is not approval.

The second gate is implementation completion. The implementer must finish the
loaded model, discovery and install propagation, active-skill injection, tests,
documentation, and roadmap update without implementing `skill_read_file`.

The third gate is validation. The implementer must run targeted tests while
developing and then the repository gate `make all` before committing the
feature. Long-running validation commands must be run through `tee` to a log
file under `/tmp`.

The fourth gate is documentation sync. `docs/roadmap.md`,
`docs/users-guide.md`, `docs/agent-skills-support.md`,
`docs/axinite-architecture-overview.md`, and `FEATURE_PARITY.md` must be
checked and updated where the shipped or documented behaviour changes.

## Constraints

- Do not implement `skill_read_file`, raw file reads, binary asset responses, or
  any new model-callable tool in this slice. Those belong to roadmap item
  `1.3.4`.
- Do not widen the generic filesystem surface. The model may receive stable
  bundle-relative metadata, but it must not receive arbitrary absolute paths as
  instructions for direct access.
- Preserve compatibility with raw single-file `SKILL.md` installs and existing
  flat and nested discovery layouts.
- Preserve existing trust semantics. `SkillTrust`, `attenuate_tools()`, the
  installed-skill downgrade text, and tool visibility must not change except
  for tests that prove they are unchanged.
- Keep skill identity and path policy in `src/skills/`. Transport-specific
  adapters in web handlers and tools must not derive their own canonical root
  rules.
- Use typed Rust fields or small value types for root and entrypoint metadata;
  avoid adding unstructured parallel strings when a named type or struct can
  make the invariant clearer.
- Avoid new runtime dependencies. The current standard library, `camino` if it
  is already available in the crate, and existing workspace crates are enough
  for path metadata. Adding `rstest-bdd` and `rstest-bdd-macros` as
  development dependencies is explicitly in scope if they are not already
  present.
- Use `rstest` fixtures for shared test setup and parameterized cases.
- Add an `rstest-bdd` harness for this slice. The harness may be small, but it
  must establish the pattern for behavioural skill tests with a `.feature`
  file, step definitions, and at least one scenario that fails before the
  runtime metadata change and passes after it.
- Do not update `docs/roadmap.md` to mark `1.3.3` done until the approved
  implementation and gates have passed.

If satisfying the objective requires violating a constraint, stop, document the
conflict in `Decision Log`, and ask for direction.

## Tolerances (exception triggers)

- Scope: if implementation needs changes to more than 15 non-test source files
  or more than 700 net non-documentation lines, stop and escalate.
- Interface: if a public HTTP, command-line, tool schema, or database contract
  must change, stop and escalate. Adding fields to internal `LoadedSkill` and
  model-facing active-skill prompt blocks is in scope.
- Dependencies: if a new crate dependency is required, stop and escalate.
- Storage: if durable database migrations or persisted settings changes appear
  necessary, stop and escalate. This task is about the loaded in-memory runtime
  model and filesystem-root metadata.
- Security: if a step would expose raw absolute paths to the language model as
  actionable file paths, stop and redesign the injection shape.
- Tests: if targeted tests still fail after three focused fix attempts, stop
  and record the failing command and failure summary.
- Ambiguity: if canonical root has multiple plausible meanings that materially
  affect `skill_read_file` in `1.3.4`, stop and present the options. The default
  assumption is that the canonical root is the final installed or discovered
  directory that contains `SKILL.md`, and the entrypoint is bundle-relative
  `SKILL.md`.

## Risks

- Risk: installed-dir discovery currently passes `SkillSource::User` for
  installed skills.
  Severity: medium.
  Likelihood: high.
  Mitigation: keep trust as the authority for attenuation and add explicit root
  metadata rather than overloading `SkillSource`. Update documentation if the
  source variant remains imprecise.

- Risk: staged install validates `SKILL.md` before the staged directory is
  renamed into its final location.
  Severity: medium.
  Likelihood: high.
  Mitigation: ensure the prepared `LoadedSkill` records the final root path, not
  the temporary staging path, before `commit_install()` inserts it.

- Risk: active-skill injection could accidentally disclose absolute local paths.
  Severity: medium.
  Likelihood: medium.
  Mitigation: inject a stable logical root such as `root="."` and
  `entry="SKILL.md"` plus the canonical skill identifier. Keep the filesystem
  root inside runtime state for future tool resolution.

- Risk: changing `LoadedSkill` breaks many existing test fixture constructors.
  Severity: low.
  Likelihood: high.
  Mitigation: add a small fixture helper or constructor so tests do not
  duplicate every field, then update existing constructors mechanically.

- Risk: `rstest-bdd` is documented but not currently wired into source tests.
  Severity: low.
  Likelihood: medium.
  Mitigation: make the first harness part of this work rather than deferring it.
  Keep it narrow: one feature file for active skill context, local step
  definitions that reuse existing dispatcher or registry fixtures, and no broad
  behaviour-testing framework beyond the documented `rstest-bdd` macros.

## Progress

- [x] (2026-04-30 19:17Z) Loaded `execplans`, `hexagonal-architecture`,
  `leta`, `rust-router`, `rust-types-and-apis`, `arch-crate-design`, and
  `commit-message` skills relevant to planning and future implementation.
- [x] (2026-04-30 19:17Z) Checked branch `feat/skill-roots-execplan`; it is
  task-specific and not the main branch.
- [x] (2026-04-30 19:17Z) Used a Wyvern agent team for read-only
  reconnaissance across the roadmap/RFC, skill code paths, and
  documentation/testing context.
- [x] (2026-04-30 19:17Z) Reviewed `docs/roadmap.md`, RFC 0003,
  `docs/agent-skills-support.md`, the current skills registry, staged install,
  bundle validator, and dispatcher injection code.
- [x] (2026-04-30 20:03Z) Revised the draft plan to make an `rstest-bdd`
  harness mandatory for this feature rather than a proportionality exception.
- [ ] Obtain explicit approval for this ExecPlan.
- [ ] Implement the loaded model and propagation changes.
- [ ] Add targeted unit and behavioural tests.
- [ ] Update user-facing and maintainer-facing documentation.
- [ ] Run validation gates, commit the implementation, and mark roadmap item
  `1.3.3` done.

## Surprises & discoveries

- Observation: `docs/axinite-architecture-summary.md` was requested as a
  reference but does not exist in this checkout.
  Evidence: `find docs -maxdepth 2 -iname '*summary*' -o -iname '*architecture*'`
  returned `docs/axinite-architecture-overview.md` and related architecture
  documents, but no summary file.
  Impact: use `docs/axinite-architecture-overview.md` as the available
  high-level architecture source and note the missing file in final
  implementation records if it remains absent.

- Observation: `.skill` archive validation and upload install support already
  exist from roadmap items `1.3.1` and `1.3.2`.
  Evidence: `src/skills/bundle/`, `src/skills/registry/materialize.rs`,
  `src/skills/registry/staged_install.rs`, and
  `tests/channels/skills_upload.rs` are present.
  Impact: this plan should build on those seams and avoid re-implementing
  archive policy.

- Observation: installed skills are loaded with `SkillTrust::Installed`, but
  installed discovery and staged install currently use `SkillSource::User`.
  Evidence: `SkillRegistry::discover_all()` calls `discover_from_dir()` with
  `SkillSource::User` for `installed_dir`, and `prepare_install_to_disk()` sets
  `let source = SkillSource::User(final_dir.clone())`.
  Impact: root metadata must not depend on `SkillSource` alone for future
  read-file resolution.

- Observation: the repository has no obvious existing `rstest-bdd` Rust harness
  or `.feature` files.
  Evidence: `find . -path '*rstest*bdd*' -o -name '*.feature'` found only
  `docs/rstest-bdd-users-guide.md`.
  Impact: this feature should add the first narrow harness, using the existing
  guide as the local pattern, so behavioural tests are not deferred again.

## Decision log

- Decision: keep this plan scoped to runtime metadata and active-skill
  injection, not skill file reads.
  Rationale: roadmap item `1.3.3` is a prerequisite for `1.3.4`; implementing
  the read tool here would widen the runtime surface before stable roots are
  proven.
  Date/Author: 2026-04-30, planning agent with Wyvern reconnaissance.

- Decision: record the final filesystem root in runtime state but inject only
  bundle-relative metadata into the model-facing prompt.
  Rationale: RFC 0003 requires the runtime to retain the canonical root for
  `skill_read_file`, while the model should reason in terms of stable
  skill-scoped paths rather than host-local absolute paths.
  Date/Author: 2026-04-30, planning agent.

- Decision: prefer a small `LoadedSkillLocation` or equivalent typed field over
  loose root and entrypoint strings.
  Rationale: `rust-types-and-apis` guidance applies here: invalid states such as
  "entrypoint without root" should be hard to construct.
  Date/Author: 2026-04-30, planning agent.

- Decision: treat `docs/axinite-architecture-overview.md` as the architecture
  reference because `docs/axinite-architecture-summary.md` is absent.
  Rationale: the repository documentation index names the overview as the
  current top-level runtime shape.
  Date/Author: 2026-04-30, planning agent.

- Decision: require an `rstest-bdd` harness in the implementation of `1.3.3`.
  Rationale: deferring BDD as disproportionate would leave this area with no
  behavioural-test pattern. A small harness can prove the externally relevant
  prompt contract without expanding the runtime feature scope.
  Date/Author: 2026-04-30, user preference captured by planning agent.

## Outcomes & retrospective

This plan is a draft and has not been implemented. Update this section after
each implementation milestone with what changed, what was validated, and any
remaining gaps. At completion, compare the shipped behaviour against roadmap
item `1.3.3` and RFC 0003's runtime model.

## Context and orientation

Start with `docs/contents.md`, `docs/welcome-to-axinite.md`, and
`docs/axinite-architecture-overview.md` for repository direction and the
current runtime shape. The requested `docs/axinite-architecture-summary.md`
does not exist in this checkout.

The governing feature documents are `docs/roadmap.md` and
`docs/rfcs/0003-skill-bundle-installation.md`. Roadmap item `1.3.3` says to
persist canonical skill roots in the loaded skill model. RFC 0003 says the
loaded model must retain the canonical skill identifier exposed to the model,
the canonical skill root directory on disk, and whether the skill was installed
as a single file or as a bundle. It also says active-skill injection should
include the skill name, canonical identifier and bundle-relative entrypoint,
and the full `SKILL.md` content.

The relevant code is concentrated in these files:

- `src/skills/mod.rs` defines `LoadedSkill`, `SkillSource`, `SkillTrust`,
  `SkillManifest`, `escape_xml_attr()`, and `escape_skill_content()`.
- `src/skills/registry/loading.rs` owns `load_and_validate_skill()`, which
  reads `SKILL.md`, parses the manifest, validates gating and prompt budget,
  computes the content hash, compiles activation patterns, and constructs
  `LoadedSkill`.
- `src/skills/registry/discovery.rs` scans flat `dir/SKILL.md` and nested
  `dir/<skill-name>/SKILL.md` layouts and calls `load_and_validate_skill()`.
- `src/skills/registry/materialize.rs` turns raw markdown, downloaded bytes, or
  archive bytes into an `InstallArtifact` containing files staged under a final
  install directory name.
- `src/skills/registry/staged_install.rs` writes a staged tree, validates the
  staged `SKILL.md`, and commits the install into the registry after a rename.
- `src/skills/bundle/mod.rs` and `src/skills/bundle/path.rs` validate passive
  `.skill` archives and produce bundle-relative paths such as `SKILL.md`,
  `references/usage.md`, and `assets/logo.txt`.
- `src/agent/dispatcher/core.rs` owns
  `Agent::build_skill_context_block()`, which currently injects `<skill>`
  blocks with `name`, `version`, and `trust`.
- `src/agent/dispatcher/tests/skills.rs`, `src/skills/registry/tests/`, and
  `src/skills/attenuation.rs` contain the closest tests and fixture
  constructors that must be updated.

Useful documentation references for implementation are
`docs/rust-testing-with-rstest-fixtures.md`,
`docs/reliable-testing-in-rust-via-dependency-injection.md`,
`docs/rust-doctest-dry-guide.md`,
`docs/complexity-antipatterns-and-refactoring-strategies.md`, and
`docs/rstest-bdd-users-guide.md`.

Relevant skills for implementers are:

- `leta` for symbol navigation and reference checks. If rust-analyzer is not
  available, use `rg` for literal searches and record the fallback.
- `rust-router` before Rust implementation work, then `rust-types-and-apis` for
  the loaded model shape.
- `arch-crate-design` for deciding whether the identity type belongs in
  `src/skills/mod.rs` or a narrower skills submodule.
- `hexagonal-architecture` to keep policy in the skills subsystem and adapters
  thin.
- `commit-message` when committing the approved implementation.

## Plan of work

Stage A is approval and baseline verification. Confirm that this ExecPlan has
been approved. Re-read `docs/roadmap.md` item `1.3.3`, RFC 0003 sections
`Reference Model`, `Runtime Model Interface`, `Data Model Changes`, and
`Rollout Plan`, then inspect the current code paths listed above. Run a
targeted baseline test command before editing so failures are not mistaken for
regressions:

```bash
BRANCH_SLUG=$(git branch --show-current | tr '/' '_')
cargo test skills::registry::tests skills::attenuation \
  agent::dispatcher::tests::skills --lib 2>&1 \
  | tee /tmp/baseline-skill-roots-axinite-${BRANCH_SLUG}.out
```

Stage B is the loaded model shape. In `src/skills/mod.rs`, add a small typed
location model, for example `LoadedSkillLocation`, that stores the canonical
runtime identifier, the canonical filesystem root, the bundle-relative
entrypoint, and the packaging mode. The exact names may change during
implementation, but the type must make these concepts explicit:

```rust
pub enum SkillPackageKind {
    SingleFile,
    Bundle,
}

pub struct LoadedSkillLocation {
    pub skill: String,
    pub root: PathBuf,
    pub entrypoint: PathBuf,
    pub package_kind: SkillPackageKind,
}
```

If `PathBuf` is retained, document that `root` is for runtime use only and
`entrypoint` is bundle-relative. If a narrower path type already exists or
`camino::Utf8PathBuf` is already idiomatic in this crate, use it only if it
reduces conversions rather than adding churn. Add accessor methods on
`LoadedSkill`, such as `skill_identifier()`, `skill_root()`,
`skill_entrypoint()`, and `package_kind()`, if they keep call sites clear.

Stage C is propagation from discovery and staged install. Change
`load_and_validate_skill()` so callers pass enough context to construct
location metadata. For flat layout `dir/SKILL.md`, the runtime root should be
the directory containing that file and the entrypoint should be `SKILL.md`. For
nested layout `dir/<skill-name>/SKILL.md`, the runtime root should be
`dir/<skill-name>` and the entrypoint should be `SKILL.md`. For staged
installs, the prepared `LoadedSkill` must record `final_dir`, not `staged_dir`,
even though the staged `SKILL.md` is what was parsed. The package kind should
come from `InstallArtifact`: raw markdown is `SingleFile`, validated archive
bytes are `Bundle`. Reload must rediscover the same metadata from disk.

Stage D is active-skill injection. Update
`Agent::build_skill_context_block()` in `src/agent/dispatcher/core.rs` so each
selected skill block includes stable escaped attributes for the canonical
identifier and bundle-relative layout. Keep the existing name, version, trust
label, content escaping, and installed-skill downgrade suffix. A valid target
shape is:

```plaintext
<skill name="deploy-docs" skill="deploy-docs" root="." entry="SKILL.md" version="1.0.0" trust="INSTALLED">
...escaped prompt body...
</skill>
```

The exact attribute order may follow the repository's snapshot style, but tests
must assert that `skill`, `root`, and `entry` cannot be omitted. Do not include
the absolute filesystem root in the prompt block. If a field needs to express
bundle versus single-file mode to the model, prefer a non-sensitive attribute
such as `package="bundle"`.

Stage E is tests. Add or update `rstest` coverage in
`src/skills/registry/tests/discovery.rs`,
`src/skills/registry/tests/install.rs`, `src/agent/dispatcher/tests/skills.rs`,
and any helper constructors affected by the new fields. Cover:

- flat single-file discovery records root as the directory containing
  `SKILL.md`, entrypoint `SKILL.md`, and package kind `SingleFile`.
- nested discovery records root as the skill directory and entrypoint
  `SKILL.md`.
- bundle staged install records final installed root, not temporary staged
  root, and package kind `Bundle`.
- downloaded markdown install records package kind `SingleFile`.
- reload preserves the same metadata shape after clearing memory.
- active-skill context includes escaped `skill`, `root`, and `entry` metadata.
- prompt content cannot break out of `<skill>` and metadata cannot inject XML
  attributes.
- tool attenuation behaviour is unchanged for trusted, installed, and mixed
  active skills.

Add the first `rstest-bdd` behavioural harness for this feature if none exists.
Add the development dependencies in `Cargo.toml` if they are absent:

```toml
rstest-bdd = "0.5.0"
rstest-bdd-macros = { version = "0.5.0", features = [
    "compile-time-validation",
] }
```

Create a focused feature file, for example
`src/agent/dispatcher/tests/features/active_skill_context.feature`, with one
scenario named "Selected bundle skill exposes stable bundle-relative metadata".
The scenario should be business-readable and should describe the observable
contract:

```gherkin
Feature: Active skill bundle metadata

  Scenario: Selected bundle skill exposes stable bundle-relative metadata
    Given an installed bundled skill with supporting files
    When the skill is selected for an agent turn
    Then the active skill context names the skill identifier
    And the active skill context names SKILL.md as the entrypoint
    And the active skill context does not expose the filesystem root
```

Add matching step definitions in a small test module, for example
`src/agent/dispatcher/tests/skill_bundle_context_bdd.rs`, using
`rstest_bdd_macros::{given, when, then, scenario}` and any `rstest` fixtures
that keep setup readable. It is acceptable for the BDD harness to drive the
dispatcher context-rendering seam directly rather than starting the whole
server, because roadmap item `1.3.3` changes the active-skill prompt contract,
not an HTTP endpoint. The step assertions must fail against the old
`<skill name="..." version="..." trust="...">` rendering and pass only when
`skill`, bundle-relative `entry`, and non-sensitive `root` metadata are
present without leaking the absolute filesystem root.

Stage F is documentation. Update `docs/users-guide.md` so users know that
bundle installs now retain stable runtime identity and entrypoint metadata, but
runtime file reads remain unavailable until `skill_read_file` lands. Update
`docs/agent-skills-support.md` sections 3, 4, 8, and 10 so the maintainer
reference no longer says loaded skills lack any root or entrypoint metadata.
Update `docs/axinite-architecture-overview.md` if its skills phase summary
needs the new runtime model. Check `FEATURE_PARITY.md` for skills rows that
should mention stable bundle-root metadata. After the feature passes all gates,
mark roadmap item `1.3.3` in `docs/roadmap.md` as done.

Stage G is final validation, refactoring, and commit. Run targeted tests, then
`make all`, Markdown linting for changed docs, and `git diff --check`. If the
functional change leaves awkward fixture duplication or a long constructor,
perform a small refactor as a separate commit after the feature commit and
rerun the relevant gates.

## Concrete steps

All commands run from the repository root:

```bash
cd /home/leynos/.lody/repos/github---leynos---axinite/worktrees/ce3e0442-2b84-4d59-865a-8c49cca63415
```

Before implementation, confirm branch and status:

```bash
git branch --show-current
git status --short
```

Expected branch:

```plaintext
1-3-3-persist-canonical-skill-roots-in-the-loaded-model
```

Run targeted baseline tests:

```bash
BRANCH_SLUG=$(git branch --show-current | tr '/' '_')
cargo test skills::registry::tests skills::attenuation \
  agent::dispatcher::tests::skills --lib 2>&1 \
  | tee /tmp/baseline-skill-roots-axinite-${BRANCH_SLUG}.out
```

During implementation, run narrower tests after each stage. Useful commands are:

```bash
BRANCH_SLUG=$(git branch --show-current | tr '/' '_')
cargo test skills::registry::tests::discovery --lib 2>&1 \
  | tee /tmp/test-discovery-skill-roots-axinite-${BRANCH_SLUG}.out
cargo test skills::registry::tests::install --lib 2>&1 \
  | tee /tmp/test-install-skill-roots-axinite-${BRANCH_SLUG}.out
cargo test agent::dispatcher::tests::skills --lib 2>&1 \
  | tee /tmp/test-dispatcher-skill-roots-axinite-${BRANCH_SLUG}.out
cargo test agent::dispatcher::tests::skill_bundle_context_bdd --lib 2>&1 \
  | tee /tmp/test-bdd-skill-roots-axinite-${BRANCH_SLUG}.out
cargo test skills::attenuation --lib 2>&1 \
  | tee /tmp/test-attenuation-skill-roots-axinite-${BRANCH_SLUG}.out
```

Before committing the feature, run the repository gate:

```bash
BRANCH_SLUG=$(git branch --show-current | tr '/' '_')
make all 2>&1 | tee /tmp/make-all-axinite-${BRANCH_SLUG}.out
```

For changed Markdown files, run:

```bash
BRANCH_SLUG=$(git branch --show-current | tr '/' '_')
bunx markdownlint-cli2 \
  docs/roadmap.md \
  docs/users-guide.md \
  docs/agent-skills-support.md \
  docs/axinite-architecture-overview.md \
  docs/execplans/1-3-3-persist-canonical-skill-roots-in-the-loaded-model.md \
  2>&1 | tee /tmp/markdownlint-skill-roots-axinite-${BRANCH_SLUG}.out
git diff --check 2>&1 \
  | tee /tmp/diff-check-skill-roots-axinite-${BRANCH_SLUG}.out
```

If `docs/axinite-architecture-overview.md` is unchanged, omit it from the
Markdown lint command. If additional Markdown files are changed, include them.

Use the `commit-message` skill and commit with `git commit -F` from a temporary
message file. Do not use `git commit -m`.

## Validation and acceptance

The feature is accepted only when the following behaviour is true:

- A nested skill directory loaded through discovery records a canonical runtime
  root equal to the directory containing its `SKILL.md`, an entrypoint equal to
  `SKILL.md`, and a stable skill identifier equal to the loaded skill name.
- A `.skill` bundle installed through the staged install path records its final
  installed directory as the root after commit, not the temporary staged
  directory.
- A raw `SKILL.md` install remains compatible and records an equivalent
  single-file location.
- `Agent::build_skill_context_block()` includes model-facing stable metadata
  for the skill identifier and bundle-relative entrypoint while preserving the
  existing prompt-body escaping and installed-skill disclaimer.
- An `rstest-bdd` feature and test module describe and verify the active-skill
  prompt contract for a selected bundled skill, including the absence of an
  absolute filesystem root in model-facing context.
- Existing trust attenuation is unchanged: installed skills still restrict the
  tool list to the read-only allowlist, and trusted skills do not.
- User-facing and maintainer-facing documentation describe the same current
  behaviour: stable runtime root metadata exists, but `skill_read_file` does
  not yet exist.
- `docs/roadmap.md` marks item `1.3.3` done only after implementation and
  validation pass.

Quality gates:

- Targeted `cargo test` commands for skills registry, dispatcher skills tests,
  `rstest-bdd` active-skill context, and attenuation pass.
- `make all` passes.
- `bunx markdownlint-cli2` passes for changed Markdown files.
- `git diff --check` passes.

No performance benchmark is required. This change adds small metadata fields
and prompt attributes on already selected skills; it does not add a hot path
loop, database query, network call, or archive parsing step.

No property test, Kani model, or Verus proof is required unless the
implementation introduces a non-trivial path-normalization invariant beyond the
existing bundle validator. If new normalization logic is added for
bundle-relative paths, add parameterized `rstest` cases first and consider a
small property test only if the input space is broader than fixed archive
shapes.

## Idempotence and recovery

The implementation steps are safe to repeat. Running discovery, install tests,
and dispatcher tests repeatedly should create only temporary test directories
that are cleaned up by test fixtures.

If staged install changes produce a commit failure in tests, use
`SkillRegistry::cleanup_prepared_install()` rather than deleting directories by
hand in production code. Manual cleanup of temporary test directories is only
needed if a test process is interrupted.

If `make all` fails because another Cargo job holds the shared package cache,
wait for Cargo's lock rather than creating an isolated Cargo cache. Do not kill
other agents' processes.

If Markdown lint fails on pre-existing unrelated lines, do not rewrite unrelated
documents broadly. Record the failure, fix only changed lines where possible,
and escalate if the gate cannot be made meaningful without unrelated churn.

## Artifacts and notes

Wyvern planning reconnaissance found:

- roadmap item `1.3.3` is a blocker for `1.3.4` and `1.3.5`
- RFC 0003 explicitly requires canonical identifier, canonical on-disk root,
  and single-file versus bundle mode in the loaded skill model
- active-skill injection currently includes only `name`, `version`, `trust`,
  and prompt content
- installed-dir discovery currently records `SkillSource::User` despite using
  installed trust
- `docs/axinite-architecture-summary.md` is absent

Current active-skill injection looks like this in snapshots:

```plaintext
<skill name="my-skill" version="1.2.3" trust="TRUSTED">
Use <b>bold</b> & 'quotes' here
</skill>
```

The intended model-facing shape after implementation is:

```plaintext
<skill name="my-skill" skill="my-skill" root="." entry="SKILL.md" version="1.2.3" trust="TRUSTED">
Use <b>bold</b> & 'quotes' here
</skill>
```

This shape keeps the model on bundle-relative paths while the runtime retains
the filesystem root privately.

## Interfaces and dependencies

The final implementation should expose a narrow internal model in or near
`src/skills/mod.rs`. The exact names may change, but the concepts should be
equivalent to:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillPackageKind {
    SingleFile,
    Bundle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSkillLocation {
    skill: String,
    root: PathBuf,
    entrypoint: PathBuf,
    package_kind: SkillPackageKind,
}
```

`LoadedSkill` should contain the location value and provide read-only accessors.
The constructor path should ensure `entrypoint` is always bundle-relative and
never absolute. The `root` value is runtime state for future file resolution,
not prompt text.

`src/skills/registry/materialize.rs` should carry enough information from
`InstallArtifact` to staged install to distinguish markdown from bundle
installs. `src/skills/registry/staged_install.rs` should pass final-root
metadata to `load_and_validate_skill()`.

`src/skills/registry/discovery.rs` should derive location metadata from the
actual `SKILL.md` path and directory layout. It should not inspect archive
metadata during discovery; after install, the canonical installed tree on disk
is the source of truth.

`src/agent/dispatcher/core.rs` should render only safe logical attributes into
the `<skill>` block. Continue to use `escape_xml_attr()` for every attribute
value and `escape_skill_content()` for the prompt body.

No new external service, database migration, or network dependency is part of
this plan.

## Revision note

Initial draft created on 2026-04-30. The plan captures roadmap item `1.3.3`
and incorporates Wyvern read-only reconnaissance across roadmap/RFC
requirements, current skill code paths, and documentation/testing guidance.
Implementation remains blocked until the plan is explicitly approved.
