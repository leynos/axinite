# Add installation and runtime tests for bundled skills (1.3.5)

This ExecPlan (execution plan) is a living document. The sections `Constraints`,
`Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`, `Decision Log`,
and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

Roadmap item `1.3.5` closes the testing milestone for multi-file skill bundles.
The prior milestones already shipped the validator (`1.3.1`), upload and URL
install flows (`1.3.2`), persisted canonical roots (`1.3.3`), and the read-only
`skill_read_file` runtime tool (`1.3.4`). What this milestone delivers is a
deterministic, named test suite that proves the four user-visible promises in
RFC 0003: valid bundles install with every documented entry preserved,
malformed bundles are rejected at the earliest sensible boundary with typed
errors, models can lazily read each bundled file through `skill_read_file`
after a real install, and the install pipeline no longer silently drops
ancillary files in any supported install transport.

After this change, a reviewer reading the test report should see, in one place,
explicit coverage that: an uploaded `.skill` archive becomes a LoadedSkill whose
`skill_root()` contains every archive entry with identical bytes; a
download-payload install does the same; the matching `skill_read_file` reads
return either UTF-8 text inline or the typed `non_inline_asset`/
`file_too_large` payload depending on entry; and at least one parameterized
case per RFC denial code returns the documented HTTP error and leaves no staged
temp directories behind. A property test covers the round-trip invariant over
arbitrary valid bundle shapes, and a behavioural test names the lazy-read
journey in a `.feature` file so the contract is legible to non-Rust readers.

The implementation must not start until this plan is explicitly approved.

## Approval gates

The first gate is plan approval. A human reviewer must explicitly approve this
ExecPlan before implementation starts. Silence is not approval.

The second gate is implementation completion. The implementer must deliver the
gap-fill tests, the new behavioural scenarios, the round-trip property test,
and the updated documentation without widening any runtime surface.

The third gate is milestone review. After each major milestone, run
`coderabbit review --agent`, resolve or record every concern, and continue only
when the review has no unresolved blocking findings.

The fourth gate is validation. Run the targeted tests while developing and the
final repository gates before committing: `make check-fmt`, `make lint`,
`make test`, and the aggregate `make all` when the individual transcripts do
not satisfy the repository gate. Long-running validation commands must be run
sequentially through `tee` to log files under `/tmp`.

The fifth gate is documentation sync. Update `docs/roadmap.md`,
`docs/users-guide.md`, `docs/developers-guide.md`, and
`docs/agent-skills-support.md` before marking roadmap item `1.3.5` done.

## Repository orientation

Start with `AGENTS.md`, `docs/contents.md`, `docs/welcome-to-axinite.md`, and
`docs/axinite-architecture-overview.md`. These describe the repository rules,
product direction, and top-level runtime shape. `docs/roadmap.md` defines item
`1.3.5` and its success rule:

> Tests cover valid bundles, malformed bundles, and lazy bundled-file
> reads, and prove that installation no longer drops ancillary files.

`docs/rfcs/0003-skill-bundle-installation.md` is the design authority. Read the
`Problem`, `Reference Model`, `Validation And Extraction`,
`Runtime Model Interface`, `Testing`, and `Rollout Plan` sections. The RFC's
`Testing` section enumerates the unit and behavioural cases that this milestone
is responsible for completing.

The four prior execplans capture the implementations under test:

- `docs/execplans/1-3-1-implement-skill-archive-validation-and-extraction.md`
- `docs/execplans/1-3-2-extend-skill-installation-flows-bundles.md`
- `docs/execplans/1-3-3-persist-canonical-skill-roots-in-the-loaded-model.md`
- `docs/execplans/1-3-4-skill-read-file-interface.md`

Existing implementation entry points the new tests will exercise:

- `src/skills/bundle/mod.rs` and `src/skills/bundle/path.rs`: archive
  validator (`validate_skill_archive`, `SkillBundleError`,
  `looks_like_skill_archive`, `ValidatedSkillBundle`).
- `src/skills/registry/staged_install.rs`: install pipeline
  (`SkillInstallPayload`, `prepare_install_to_disk`, `commit_install`,
  `cleanup_prepared_install`, `PreparedSkillInstall`,
  `CommitPreparedInstallError`).
- `src/skills/registry.rs` and `src/skills/registry/loading.rs`:
  registry lookup (`SkillRegistry::find_by_name`, `SkillRegistry::has`,
  `SkillRegistry::commit_loaded_skill`).
- `src/skills/file_read.rs` and `src/skills/file_read/io.rs`:
  read-only policy (`read_skill_file`, `SkillReadFileResponse`,
  `SkillReadFileErrorCode`).
- `src/skills/mod.rs`: domain model (`LoadedSkill`,
  `LoadedSkillLocation`, `SkillPackageKind`, `LoadedSkillParts`).
- `src/tools/builtin/skill_tools/read_file.rs` and
  `src/tools/builtin/skill_tools/install.rs`: built-in tool adapters.
- `src/channels/web/handlers/skills.rs`,
  `src/channels/web/handlers/install_helpers.rs`, and
  `src/channels/web/handlers/skills/tests/`: HTTP install transport.
- `src/tools/builtin/skill_fetch/`: URL fetch helpers with the
  HTTPS/SSRF policy; its own URL-policy tests live in
  `src/tools/builtin/skill_fetch/tests.rs`.

Relevant existing test bases to consult and extend rather than duplicate:

- `src/skills/bundle/tests.rs`: validator unit and parameterized
  negative cases. Already extensive.
- `src/skills/registry/tests/install.rs`: install pipeline lifecycle,
  with `BundleInstallFixture` and `build_bundle_archive`.
- `src/skills/registry/tests/discovery.rs`: discovery and layout
  coverage; `test_load_skill_layout` already covers single-file and
  subdirectory layouts.
- `src/skills/registry/tests/prop_tests.rs`: existing `proptest` cases
  for `LoadedSkillLocation`. The new round-trip property test should share this
  module or live next to it.
- `src/skills/file_read/tests.rs`: unit, property, and snapshot
  coverage for path policy and stable JSON shapes.
- `src/tools/builtin/skill_tools/tests.rs` plus
  `src/tools/builtin/skill_tools/features/skill_read_file.feature`:
  tool-adapter unit tests and the two existing `rstest-bdd` scenarios.
- `src/agent/dispatcher/tests/skill_bundle_context_bdd.rs` plus
  `src/agent/dispatcher/tests/features/active_skill_context.feature`:
  active-skill context BDD that exercises the prompt boundary.
- `tests/channels/skills_upload.rs`: web gateway multipart upload
  integration test (only the happy path today).
- `tests/e2e/scenarios/test_skills.py`: Python Playwright e2e for the
  Skills UI tab; uses ClawHub and skips if unreachable.

The implementation should load these skills before editing:

- `leta`, for symbol navigation and reference checks.
- `rust-router`, then the smallest relevant Rust follow-on skill. For
  this work, expect `rust-types-and-apis` for fixture shapes, `rust-errors` for
  typed denial assertions, and `rust-async-and-concurrency` if registry lock
  semantics need exercising under proptest.
- `hexagonal-architecture`, as test-boundary discipline: domain tests
  in `src/skills/`, adapter tests in `src/tools/` and `src/channels/web/`, and
  externally observable journey tests in `tests/`.
- `nextest`, when filtering tests during development.
- `firecrawl` only if a question about RFC interpretation requires
  external evidence.
- `commit-message` and `pr-creation`, when committing or updating the
  draft pull request.

Relevant docs to consult during design:

- `docs/rust-testing-with-rstest-fixtures.md`: fixture-first patterns
  the new tests must match.
- `docs/reliable-testing-in-rust-via-dependency-injection.md`: how to
  share registry-backed state without environment mutation.
- `docs/rust-doctest-dry-guide.md`: doctest conventions when the new
  helpers grow public docs.
- `docs/complexity-antipatterns-and-refactoring-strategies.md`: keeps
  the helper modules small and focussed.
- `docs/rstest-bdd-users-guide.md`: governs the new
  `.feature` files and step definitions.

## Constraints

- Do not change any production code in `src/skills/`, `src/tools/`, or
  `src/channels/web/` beyond minimal test-only seams already exposed (for
  example, `SkillRegistry::with_installed_dir`, fixture builders, and the
  existing `test_support` module). If a test demands a new public seam in
  production code, stop and document the request in `Decision Log` before
  adding it.
- Do not weaken or rewrite the HTTPS/SSRF policy in
  `src/tools/builtin/skill_fetch/url_policy.rs`. The URL install path is
  exercised at the `SkillInstallPayload::DownloadedBytes` layer, not by
  standing up an HTTPS mock that bypasses the policy.
- Keep validator and policy invariants in `src/skills/`. Tests in
  `src/tools/` or `src/channels/web/` must call shared helpers rather than
  reimplementing archive policy.
- Do not introduce new runtime dependencies. `zip`, `tempfile`,
  `serde`, `serde_json`, `tokio`, `rstest`, `rstest-bdd`, `rstest-bdd-macros`,
  `insta`, `proptest`, `mime_guess`, and `reqwest` are already available.
- New test-only dependencies require a short decision-log entry
  explaining why existing crates are insufficient.
- Use `rstest` fixtures for shared setup and `#[rstest]` parameterized
  cases for table-driven coverage. Use `rstest-bdd` (Rust) for behavioural
  scenarios. Do not add `pytest-bdd` to the Python e2e package for this
  milestone.
- Use `proptest` for the round-trip and path-validation invariants.
  Do not add Kani or Verus; the invariants are bounded over archive entries,
  not unbounded mathematical lemmas.
- Use `insta` snapshot tests only where the assertion is genuinely
  about a stable JSON wire shape (for example, the HTTP install response body
  or a typed denial payload). Do not add snapshots for values that an
  `assert_eq!` already covers precisely.
- Property-test archives must stay small (per-entry size bounded by
  `MAX_BUNDLE_FILE_BYTES`; total entries bounded so a shrinking run completes
  in well under one second). Bound the proptest case count so the suite stays
  within the existing nextest budget.
- Tests that exercise the real filesystem-read path through
  `read_skill_file` must be gated on `target_os = "linux"`, mirroring
  `src/skills/file_read/tests.rs`, and the non-Linux variant must assert the
  documented fail-closed `IoError`.
- Do not mark roadmap item `1.3.5` done until tests, documentation,
  `coderabbit review --agent`, and the final gates pass.

If satisfying the objective requires violating a constraint, stop, document the
conflict in `Decision Log`, and ask for direction.

## Tolerances

- Scope: if implementation needs more than 12 new or modified test
  files, or more than 1,200 net non-documentation lines, stop and reassess the
  boundary between gap-fill and rewrite.
- Production code change: if any new test demands a non-trivial
  production-code change, stop and ask for approval before continuing. Trivial
  seams (a fixture helper behind `#[cfg(test)]` or under a `test_support`
  module) are in scope.
- Test cost: if any single new test wall-clock exceeds 5 seconds in
  the default nextest profile, stop and refactor before adding more cases.
- Property-test budget: configure the round-trip proptest with
  `ProptestConfig::with_cases(32).max_shrink_iters(64)` and aim for a
  wall-clock under 5 seconds on the reference hardware. If shrinking exceeds
  that budget, revisit the generator constraints rather than disabling
  shrinking.
- CodeRabbit cadence: if `coderabbit review --agent` cannot reach a
  clean review after three retries because of upstream rate limiting, capture
  the latest transcript path, record the gap in `Surprises & Discoveries`, and
  continue. Do not re-run in a blocking loop.
- Iterations: if targeted tests still fail after three focussed fix
  attempts, stop and document the failing command, log path, and failure
  summary.
- Validation time: if any single gate approaches the 1200-second
  command limit, stop, capture the log, and split the next run into smaller
  documented pieces.
- Ambiguity: if multiple valid interpretations of "lazy bundled-file
  read" affect what is exercised at the integration boundary (for example,
  whether to drive `read_skill_file` through the `SkillReadFileTool` adapter or
  directly), choose the path that exercises the most adapter glue and document
  the choice in `Decision Log`.
- New behavioural file: at most two new `.feature` files; if a third
  appears necessary, stop and check whether existing files should be extended
  instead.

## Risks

- Risk: the round-trip property test is the most novel addition.
  Generating valid bundle archives in `proptest` may be slow or produce
  shrinking traces that are hard to read. Severity: medium. Likelihood: medium.
  Mitigation: keep generators narrow (one root, bounded entry count, bounded
  content size, ASCII-only file stems), prefer combinator composition over
  handwritten `Strategy::new`, and assert on a concrete map of
  `relative_path -> bytes` rather than ordering.

- Risk: tests that span install plus runtime read end up duplicating
  the fixture surface of two test bases. Severity: medium. Likelihood: high.
  Mitigation: factor a shared `installed_bundle_fixture` into
  `src/skills/test_support.rs` (already public to the crate) so the
  registry-install tests, file-read tests, and tool-adapter tests can all
  consume the same prepared bundle without copying fixtures.

- Risk: web gateway negative tests may rely on registry state that
  the existing happy-path test does not exercise, creating ordering fragility.
  Severity: low. Likelihood: medium. Mitigation: keep one `TestGatewayBuilder`
  per test, build all registries from `tempfile::TempDir`, and assert on
  `install_dir` contents directly after each failing request.

- Risk: behavioural `.feature` scenarios fall out of sync with the
  step definitions when other contributors rename steps. Severity: low.
  Likelihood: low. Mitigation: keep the `.feature` and its step file colocated
  under the same directory, and add a comment in the step file pointing back to
  the feature filename.

- Risk: macOS or Windows runners cannot exercise the read path
  because the implementation uses Linux `openat2`. Severity: medium.
  Likelihood: high. Mitigation: gate read-path assertions on
  `target_os = "linux"` and assert `SkillReadFileErrorCode::IoError` on
  non-Linux, matching `src/skills/file_read/tests.rs`. CI's primary Linux
  pipeline still exercises the inline-content path end to end.

- Risk: the URL install transport is HTTPS-only and rejects private
  hosts; standing up an HTTPS mock with a public-looking name to exercise the
  URL→install path is out of proportion for this milestone. Severity: low.
  Likelihood: medium. Mitigation: cover the URL path at the
  `SkillInstallPayload::DownloadedBytes` boundary, which is what the HTTP
  handler hands to the registry after the fetch policy passes. The fetch policy
  itself stays covered by `src/tools/builtin/skill_fetch/tests.rs`.

- Risk: a future contributor adds new variants to
  `SkillReadFileErrorCode` and the snapshot suite drifts silently. Severity:
  low. Likelihood: low. Mitigation: keep snapshot coverage scoped to the
  variants this milestone needs, and rely on the existing
  `snapshot_skill_read_file_response_shapes` parameterization for per-variant
  churn.

## Progress

- [x] Plan approved for implementation by user instruction on
  2026-06-12.
- [x] Inventory of existing skill bundle test coverage written into
  this plan's `Surprises & Discoveries`.
- [x] Stage A: extract shared `installed_bundle_fixture` and helpers
  in `src/skills/test_support.rs` if needed.
- [x] Stage B: domain-layer round-trip tests
  (`src/skills/registry/tests/install.rs`,
  `src/skills/registry/tests/prop_tests.rs`).
- [x] Stage C: runtime-read after install tests
  (`src/skills/file_read/tests.rs` or a new
  `src/skills/file_read/install_tests.rs`).
- [x] Stage D: malformed-archive transport tests
  (`tests/channels/skills_upload.rs`).
- [x] Stage E: behavioural feature for install→read journey
  (`src/tools/builtin/skill_tools/features/`).
- [x] Stage F: roadmap and documentation updates.
- [x] Stage G: `coderabbit review --agent` clean for Stages A
  through F.
- [x] Stage H: final gates (`make check-fmt`, `make lint`,
  `make test`, optionally `make all`).
- [x] Commit the approved test additions and mark `1.3.5` done.
- [x] Push the branch and refresh the draft pull request.
- [x] Rebase onto `origin/main` at `8045e754` with no conflicts and
  validate the rebased tree.
- [x] Fix post-turn markdownlint fallout in
  `docs/developers-guide.md`.
- [x] Fix the new `make audit` RustSec failures by refreshing the
  affected PostgreSQL crates in `Cargo.lock`.
- [x] Fix the Windows `--no-default-features --features libsql`
  clippy unused-code findings by gating Linux-only test helpers.
- [x] Address review feedback by documenting
  `src/skills/registry/tests/install.rs`, documenting `InstalledBundleFixture`
  and `build_bundle_archive` in the developer guide, and consolidating internal
  bundle archive builders into `src/skills/test_support.rs`.

## Surprises & discoveries

- Observation: the 2026-06-12 implementation pass confirmed the
  earlier gap analysis still matches the current tree. Registry bundle tests
  preserve a small three-file archive; file-read tests read handwritten tempdir
  fixtures; tool tests register a bundle by constructing `LoadedSkillLocation`
  directly; and the upload integration test still has one happy path only.
  Evidence: `src/skills/registry/tests/install.rs`,
  `src/skills/file_read/tests.rs`, `src/tools/builtin/skill_tools/tests.rs`, and
  `tests/channels/skills_upload.rs` were inspected before editing. Impact:
  proceed with the shared installed-bundle fixture and the planned gap-fill
  tests rather than changing production behaviour.

- Observation: the first `prop_bundle_round_trip_preserves_entries`
  implementation generated arbitrary bytes for every entry, which made some
  `.md` reference files invalid UTF-8. The validator correctly rejected those
  archives before installation. Evidence:
  `/tmp/focused-registry-stage-b-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`
  reported `InvalidUtf8Text` for a generated reference path. Impact: constrain
  the property generator to valid UTF-8 bodies so the property covers arbitrary
  valid bundle shapes, as required by this milestone.

- Observation: the concrete command in the plan that used
  `cargo nextest run -p axinite --test skills_upload` is stale. The file
  `tests/channels/skills_upload.rs` is compiled under the `channels`
  integration test target. Evidence: Cargo reported no test target named
  `skills_upload` and listed `channels` as the available target in
  `/tmp/focused-upload-stage-d-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`.
  Impact: use
  `cargo nextest run -p axinite --test channels -E 'test(/skills_upload/)'`
  for targeted gateway upload validation.

- Observation: the Stage D happy-path upload test now uses the same
  documented bundle fixture as the new read-after-upload cases, including
  `references/nested/api.md`, `assets/note.txt`, and `assets/logo.png`.
  Evidence: the first Stage D rerun failed only because the old assertion still
  expected `assets/logo.txt`; the third rerun passed all 13 `skills_upload`
  cases. Impact: keep one documented upload fixture so the gateway happy path
  and upload-to-read journey cannot drift apart.

- Observation: the existing test surface already covers a
  substantial fraction of the RFC's `Testing` checklist. Evidence:
  `src/skills/bundle/tests.rs` covers validator negatives;
  `src/skills/registry/tests/install.rs` covers payload-preserves-files for both
  `DownloadedBytes` and `ArchiveBytes` and asserts the staged directory is
  cleaned on parse failure; `src/skills/file_read/tests.rs` covers path policy,
  snapshot shapes, and Linux read behaviour; `src/tools/builtin/skill_tools/`
  covers the tool adapter and two BDD scenarios; and
  `tests/channels/skills_upload.rs` covers the multipart happy path. Impact:
  this milestone is a gap-fill, not a from-scratch test build-out. The plan
  should not duplicate cases that already pass.

- Observation: no current test exercises the full pipeline
  install→register→read. The tool-layer BDD uses a stubbed
  `LoadedSkillLocation` that points at a `tempdir`, not at the output of
  `prepare_install_to_disk + commit_install`. Evidence:
  `insert_deploy_docs_bundle` in `src/tools/builtin/skill_tools/tests.rs`
  constructs a `LoadedSkillLocation` manually rather than installing through
  `SkillRegistry`. Impact: a new round-trip test is the headline addition for
  `1.3.5`; it directly proves "installation no longer drops ancillary files"
  because each preserved entry is re-read through the production adapter.

- Observation: the HTTP gateway has no negative test coverage for
  malformed `.skill` uploads. The single existing integration test asserts only
  the happy path. Evidence: `tests/channels/skills_upload.rs` contains one test
  case and no parameterized negatives. Impact: a small `#[rstest]`
  parameterization can cover each bundle-validator negative through the gateway
  and confirm both the HTTP status code and the absence of leaked install-root
  entries.

- Observation: RFC 0003 §Canonical skill names and conflict handling
  (item 4) defines deterministic upgrade, force, and auto-rename semantics, but
  the current test base has no coverage for the collision matrix beyond the
  simple "already exists" rejection in `test_install_duplicate_rejected`.
  Evidence: searching `src/skills/registry/tests/` for `force`, `upgrade`, and
  `auto_rename` returns no hits. Impact: this is an RFC gap, not a `1.3.5`
  deliverable, because the underlying behaviour was not added in milestones
  `1.3.1` through `1.3.4`. Recorded here so a follow-up roadmap item can pick
  it up rather than letting it disappear from view.

- Observation: existing staged-install cleanup behaviour is partially
  covered by `test_prepare_install_cleans_staged_dir_when_validation_fails`
  (invalid markdown) and
  `test_cleanup_prepared_install_removes_staged_bundle_on_commit_failure`
  (duplicate name on commit), but neither names "atomic extraction" as the
  contract being asserted. Evidence: `src/skills/registry/tests/install.rs`
  lines 230 and 281. Impact: add a one-line doc comment to each, or a single new
  `test_install_atomicity_does_not_leave_partial_skill_tree` that composes
  both failure modes and asserts the install root contains no entries with the
  canonical skill name. This makes the atomicity invariant grep-able from the
  RFC.

- Observation: the prior-art research (firecrawl) confirmed CVE
  references that the new traversal and symlink negative tests should cite.
  CVE-2018-1002200 (zip-slip) is the canonical reference for traversal entries;
  CVE-2025-29787 covers the Rust `zip` crate's own historical
  directory-traversal handling. Evidence: zip-slip disclosure at
  <https://github.com/snyk/zip-slip-vulnerability> and the Rust `zip` crate
  advisory at <https://security.snyk.io/vuln/SNYK-RUST-ZIP-9460813>. Impact:
  add module-level doc comments to the new negative-test modules citing both
  CVEs so reviewers understand why the traversal and link rejections are not
  optional cosmetic checks.

- Observation: rebasing this branch onto `origin/main` at
  `8045e754` had no conflicts. The main-branch change introduced the repository
  `make audit` gate, so the branch needed later supply-chain validation in
  addition to the original `make all` gate. Evidence: `make check-fmt`,
  `make test`, `make typecheck`, and `make lint` all passed after the rebase,
  and the branch was force-pushed with lease. Impact: the feature work stayed
  intact, and the later audit fix is recorded as post-completion hardening
  rather than a scope change to `1.3.5`.

- Observation: the post-turn markdownlint hook caught one existing
  formatting issue in `docs/developers-guide.md`: two consecutive blank lines
  near the bundled-skills documentation update. Evidence: `make markdownlint`
  reported `MD012/no-multiple-blanks` for `docs/developers-guide.md`. Impact:
  remove the extra blank line and keep this as a documentation hygiene fix
  separate from the Rust implementation.

- Observation: `make audit` failed after the branch picked up the new
  audit gate because `Cargo.lock` still resolved `tokio-postgres` `0.7.17` and
  `postgres-protocol` `0.6.11`, which are covered by RustSec advisories
  `RUSTSEC-2026-0178`, `RUSTSEC-2026-0179`, and `RUSTSEC-2026-0180`. Evidence:
  the audit output requested `tokio-postgres >=0.7.18` and
  `postgres-protocol >=0.6.12`. Impact: refresh the lockfile to
  `tokio-postgres 0.7.18`, `postgres-protocol 0.6.12`, and matching
  `postgres-types 0.2.14` without changing manifest ranges.

- Observation: the Windows clippy profile
  `--no-default-features --features libsql` compiles out Linux-only file-read
  assertions, which makes related fixture fields and imports look unused on
  that target. Evidence: CI reported unused imports in
  `src/tools/builtin/skill_tools/tests.rs` and
  `tests/channels/skills_upload.rs`, plus an unread `registry` field in
  `src/skills/test_support.rs`. Impact: gate the Linux-only helper field and
  imports with `#[cfg(target_os = "linux")]` instead of suppressing warnings.

## Decision log

- Decision: treat the user's 2026-06-12 instruction to proceed with
  implementation as satisfying the plan approval gate, while retaining the plan
  as a living document. Rationale: the execplan was already drafted and the
  direct request explicitly asked for implementation according to it.
  Date/Author: 2026-06-12 / Codex.

- Decision: scope `1.3.5` as named gap-fill plus one round-trip
  property test, rather than a wholesale rewrite of existing skill tests.
  Rationale: prior milestones already shipped solid coverage of their own
  slices. Duplicating those tests inside `1.3.5` would inflate the change
  without strengthening the contract. Date/Author: 2026-06-02 / Codex.

- Decision: cover the URL install transport at the
  `SkillInstallPayload::DownloadedBytes` boundary rather than standing up an
  HTTPS mock. Rationale: the URL-fetch policy is HTTPS-only and rejects private
  hosts, so a credible mock would either weaken the policy or introduce a new
  dependency to spoof a public hostname. The downstream install path is
  identical once bytes arrive, and the fetch policy is already tested
  independently. Date/Author: 2026-06-02 / Codex.

- Decision: gate runtime-read assertions on `target_os = "linux"`,
  matching `src/skills/file_read/tests.rs`. Rationale: `read_skill_file` uses
  Linux `openat2` and falls closed on other platforms with `IoError`. Keeping
  the same gating pattern avoids inventing a parallel platform matrix.
  Date/Author: 2026-06-02 / Codex.

- Decision: keep new `.feature` files at most two: one install→read
  scenario for the tool adapter, optionally one negative install-rejection
  scenario only if the existing dispatcher BDD cannot host it. Rationale:
  `rstest-bdd` features earn their keep when the scenario is genuinely
  externally observable. Per-case parameterization belongs in `#[rstest]`
  cases. Date/Author: 2026-06-02 / Codex.

- Decision: keep the install→read journey in table-driven
  `#[rstest] #[case]` coverage and do not add it as a `.feature` scenario.
  Rationale: the `Then` step iterates a fixed manifest of bundled paths. That
  is exactly the shape `#[rstest] #[case]` was designed for. A Gherkin scenario
  whose body is a `for` loop adds ceremony without making the contract more
  legible. The remaining new BDD addition (progressive disclosure) is genuinely
  behavioural because it asserts on a rendering boundary. Date/Author:
  2026-06-02 / Codex (after Logisphere review).

- Decision: place the progressive-disclosure scenario in
  `src/agent/dispatcher/tests/features/active_skill_context.feature` next to
  the existing scenario, rather than in
  `src/tools/builtin/skill_tools/features/`. Rationale: the assertion is about
  prompt rendering through `build_skill_context_block`, which already lives in
  the dispatcher BDD. Splitting that contract across two feature files would
  confuse future contributors. Date/Author: 2026-06-02 / Codex (after
  Logisphere review).

- Decision: the shared `installed_bundle_fixture` returns the following
  fixture by value.

  ```rust
  InstalledBundleFixture { _tempdir, registry: SkillRegistry, loaded_skill: LoadedSkill }
  ```

  Tests that need an `Arc<RwLock<SkillRegistry>>` (for example, the
  tool adapter) wrap the registry locally via
  `Arc::new(RwLock::new(fixture.registry))` at the call site. Rationale:
  returning the registry by value rules out shared lock-poisoning between
  cases. The `Arc<RwLock<_>>` wrap is a one-liner at the call site and keeps
  the fixture composable. Date/Author: 2026-06-02 / Codex (after Logisphere
  review).

- Decision: do not extend `tests/e2e/scenarios/test_skills.py` in
  this milestone. Rationale: the Python e2e harness drives Playwright against
  the Skills UI and depends on ClawHub for installs. The new `1.3.5` coverage
  is about the install pipeline contract, which is fully exercised at the Rust
  integration layer through the gateway helper. Adding a Python e2e here would
  duplicate lower-layer assertions without covering a distinct system flow.
  Date/Author: 2026-06-02 / Codex.

- Decision: use a lockfile-only patch update for the PostgreSQL audit
  findings. Rationale: the existing `Cargo.toml` SemVer ranges already permit
  the fixed patch releases, and widening manifests would create unrelated
  dependency churn. Date/Author: 2026-06-14 / Codex.

- Decision: fix target-specific unused-code findings with conditional
  compilation boundaries rather than `allow` or `expect` attributes. Rationale:
  the affected helpers are genuinely Linux-only because the file-read
  implementation uses Linux `openat2`. Target gating matches the test contract
  and keeps clippy strict on Windows. Date/Author: 2026-06-14 / Codex.

- Decision: keep roadmap item `1.3.5` marked complete while recording
  the rebase, audit, markdownlint, and Windows/libSQL lint work as follow-up
  hardening. Rationale: those changes validate and preserve the shipped
  milestone; they do not alter the milestone's runtime or test-coverage scope.
  Date/Author: 2026-06-16 / Codex.

## Outcomes & retrospective

Roadmap item `1.3.5` is complete. The test suite now covers valid bundle
installation through both staged payload variants, arbitrary small valid bundle
manifests via `proptest`, lazy reads after a real install through the domain
function and tool adapter, malformed multipart upload rejection, and
upload-to-read through the web gateway. The dispatcher BDD now also names the
progressive-disclosure contract: active bundled skills inject `SKILL.md` prompt
content but not ancillary reference or asset bytes.

The round-trip property test surfaced one useful implementation lesson during
development: generated bundle contents must model the validator contract.
Arbitrary bytes are valid for binary assets, but `.md` reference files are text
and must be valid UTF-8. Constraining the generator to valid UTF-8 bodies made
the property match the milestone's "arbitrary valid bundle shapes" requirement
instead of retesting known validator denials.

Two follow-up notes remain outside this milestone. RFC 0003's future
upgrade/force/auto-rename collision matrix is still not implemented, and the
existing `schema_helpers_ui::ui` test can be slow on a cold run, although the
final cached `make all` completed quickly.

After the milestone landed, the branch was rebased onto `origin/main` at
`8045e754` with no conflicts and force-pushed with lease. The post-rebase tree
passed `make check-fmt`, `make test`, `make typecheck`, and `make lint`.
Subsequent CI-facing hardening fixed one markdownlint issue, refreshed the
PostgreSQL lockfile entries that triggered the new `make audit` gate, and gated
Linux-only bundled-skill test helpers so the Windows libSQL clippy profile
passes without lint suppression.

## Plan of work

Stage A factors out shared install fixtures so the round-trip and behavioural
tests do not duplicate setup code. The likely landing spot is
`src/skills/test_support.rs`, which is already crate-visible to other test
modules. Add an `installed_bundle_fixture` builder that:

- accepts an iterable of `(relative_path, bytes)` entries,
- packages them through `build_bundle_archive`,
- runs them through `SkillRegistry::prepare_install_to_disk`
  followed by `commit_install`,
- returns a struct that owns the `TempDir`, the registry handle,
  and the `LoadedSkill`.

The existing `BundleInstallFixture` in `src/skills/registry/tests/fixtures.rs`
already provides the TempDir and registry plumbing; the new helper can compose
with it rather than replace it. Keep the helper under `cfg(test)` or
`test_support` so it does not leak into the release surface.

Stage B adds the round-trip and "no dropped files" regression tests in the
registry test module. Note: the existing `test_archive_payload_preserves_files`
at `src/skills/registry/tests/install.rs:34-97` already exercises both
`DownloadedBytes` and `ArchiveBytes` payload variants on a basic happy-path
bundle. This stage adds a strictly stronger regression test parameterized on
the same transports, plus the round-trip property test. Do not duplicate the
existing happy-path case.

- `test_install_preserves_references_and_assets_regression_rfc0003`:
  the named regression test for the bug the RFC fixes (the prior installer
  dropped ancillary files). Parameterize via `#[rstest]` on
  `SkillInstallPayload::DownloadedBytes` and `ArchiveBytes` so both transports
  share one assertion body. The bundle contains each archive-validator-accepted
  entry class (`SKILL.md`, `references/usage.md`, `references/nested/api.md`,
  `assets/note.txt`, `assets/logo.png` with a real PNG signature). After
  `commit_install`, the test asserts that the on-disk file set under
  `skill_root()` equals the input manifest exactly (set equality, no extras and
  no omissions), each entry is byte-for-byte equal to the input, and
  `LoadedSkill::package_kind()` is `Bundle`. The test's doc comment must
  explicitly cite RFC 0003 and the dropped-files bug so the assertion is not
  "simplified" by a future refactor.
- A new proptest `prop_bundle_round_trip_preserves_entries` in
  `src/skills/registry/tests/prop_tests.rs`. Generator strategy: fixed root
  `"deploy-docs"`, a manifest of one to eight entries drawn from a
  `HashSet<BundlePath>` to guarantee uniqueness without `prop_assume!`
  filtering. `BundlePath` is generated through `prop_oneof!` over four shape
  constructors (`references/<id>.md`, `references/<id>/<id>.md`,
  `assets/<id>.txt`, `assets/<id>.bin`) with `<id>` drawn from
  `[a-z][a-z0-9_-]{0,7}` and a deterministic numeric suffix to rule out
  collisions. Bodies bounded at 1 KiB each. A mandatory `SKILL.md` entry with
  valid YAML frontmatter is appended in every case. Use
  `zip::CompressionMethod::Stored` for the generated archive to cut per-case
  CPU. The property: after `prepare_install_to_disk + commit_install`, every
  input entry is byte-equal under `skill_root()`. Configure with
  `ProptestConfig::with_cases(32).max_shrink_iters(64)`.

Stage C adds the lazy-read coverage that bridges installation and runtime read.
Add `src/skills/file_read/install_tests.rs` (or extend the existing `tests.rs`
if it stays under 400 lines):

- `test_read_skill_file_after_install_returns_each_text_entry`:
  install a bundle through the shared fixture, then for each text entry assert
  `read_skill_file` returns `Success` with the expected MIME type and
  byte-equal content. Gate on `target_os = "linux"`.
- `test_read_skill_file_after_install_returns_non_inline_metadata_for_binary`:
  install a bundle with a real PNG asset, assert
  `SkillReadFileErrorCode::NonInlineAsset` with the documented `metadata`
  fields populated.
- `test_read_skill_file_after_install_returns_io_error_on_non_linux`:
  the non-Linux fallthrough case, gated on `cfg(not(target_os = "linux"))`.
  Install succeeds; only the read fails closed with `IoError`. The test name
  makes that split explicit so reviewers do not misread the assertion as an
  install failure.

Stage D extends the web gateway integration tests in
`tests/channels/skills_upload.rs` with a small negative-case matrix driven by a
local `MalformedKind` enum:

```rust
enum MalformedKind {
    ScriptsDir,
    ExecutableExtensionUnderAssets,
    DuplicateCaseFold,
    Traversal,
    OversizedArchive,
    MissingSkillMd,
    MultipleTopLevelPrefixes,
}
```

- `multipart_skill_bundle_upload_rejects_malformed_bundles`: an
  `#[rstest]` parameterized case over `MalformedKind` that builds a
  one-violation-per-case archive through a shared
  `build_malformed_archive(kind)` helper. Each case asserts HTTP
  `400 Bad Request`, an `invalid_skill_bundle` substring in the body, and that
  the install directory contains no entries afterwards. The module's doc
  comment must cite `CVE-2018-1002200` for the traversal case and the rationale
  for rejecting executable extensions so the lineage survives future refactors.
- `multipart_skill_bundle_upload_rejects_non_skill_filename`:
  upload a valid archive with the filename `deploy-docs.zip` and assert the
  gateway rejects on the multipart-side filename rule before touching the
  validator.
- `multipart_skill_bundle_upload_round_trip_reads_each_entry`:
  upload the happy path, then drive a `SkillReadFileTool` against the same
  registry handle for each entry and assert content and MIME type. Gate the
  read assertions on `target_os = "linux"`.

Stage E adds the behavioural coverage. The install→read journey is table-shaped
and belongs in `#[rstest]` parameterization rather than Gherkin (see Decision
Log entry on this), so the only new behavioural scenario in this milestone is
the progressive-disclosure assertion, and it lives next to the existing
dispatcher-side context BDD because it asserts on `build_skill_context_block`
rendering.

Add the scenario to
`src/agent/dispatcher/tests/features/active_skill_context.feature` and the
matching step definitions to
`src/agent/dispatcher/tests/skill_bundle_context_bdd.rs`:

```gherkin
Scenario: Activated bundle skill does not eagerly load ancillary files
  Given an installed bundled skill with a references file and an assets file
  When the skill is selected for an agent turn
  Then only SKILL.md content is injected into the active skill context
  And the references file content is absent from the rendered context block
  And the assets file content is absent from the rendered context block
```

The new `Given` step constructs a `LoadedSkill` whose `SKILL.md` prompt content
is a known token (for example, `"PROMPT-MARKER"`) and writes distinguishable
marker bytes to the referenced ancillary files on disk. The `Then` steps assert
that `PROMPT-MARKER` is present and the ancillary markers are absent from the
rendered context. This makes the progressive-disclosure contract externally
observable through the same prompt boundary the existing scenario already
exercises, with no new feature file created elsewhere.

For the install→read journey itself, add table-driven `#[rstest] #[case]`
coverage in `src/tools/builtin/skill_tools/tests.rs`:

- `test_skill_read_file_tool_after_install_returns_each_documented_entry`:
  parameterized over the manifest of documented bundled paths (`SKILL.md`,
  `references/usage.md`, `references/nested/api.md`, `assets/note.txt`). Each
  case installs the shared fixture, drives `SkillReadFileTool::execute` with
  the path, and asserts the returned JSON shape matches the installed bytes and
  MIME type. Gate on `target_os = "linux"`.
- `test_skill_read_file_tool_after_install_returns_non_inline_for_png`:
  the binary case, asserting the typed `non_inline_asset` payload through the
  tool adapter.

The matching step definitions go in `src/tools/builtin/skill_tools/tests.rs`
alongside the existing `SkillReadFileWorld`. Each `Then` step iterates over a
fixed manifest of paths so the assertion stays deterministic.

Stage F updates the documentation and roadmap:

- `docs/roadmap.md`: mark `1.3.5` as done after the gates pass.
- `docs/users-guide.md`: confirm the skill bundle authoring
  guidance still matches the verified contract; add a one-line note that bundle
  authors can rely on installation preserving every documented entry.
- `docs/agent-skills-support.md`: cross-reference the new test
  inventory so maintainers can locate the coverage map.
- `docs/developers-guide.md`: document the
  `installed_bundle_fixture` pattern in the testing section so future
  skill-related work reuses it.

Stage G runs `coderabbit review --agent`, resolves concerns, and re-runs to a
clean review.

Stage H runs the final gates sequentially through `tee` and commits through the
`commit-message` skill workflow.

## Concrete steps

All commands run from the repository root:

```bash
cd /home/leynos/.lody/repos/github---leynos---axinite/worktrees/2c85664b-da4e-495b-a4a3-4723982eb0b5
```

Confirm the branch, leta workspace, and existing test inventory before editing:

```bash
git branch --show-current
leta workspace add "$PWD"
leta grep "validate_skill_archive" -k function
leta grep "prepare_install_to_disk" -k function,method
leta refs SkillReadFileTool
```

Expected branch output, after the rename:

```plaintext
1-3-5-installation-and-runtime-tests-for-bundled-skills
```

Run targeted tests as red checks before adding code:

```bash
ACTION=red-1-3-5
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
FILTER='test(=test_install_preserves_references_and_assets_regression_rfc0003)'
FILTER="$FILTER | test(=prop_bundle_round_trip_preserves_entries)"
FILTER="$FILTER | test(=test_read_skill_file_after_install_returns_each_text_entry)"
cargo nextest run --workspace --no-fail-fast -E "$FILTER" 2>&1 | tee "$LOG"
```

Expect those filters to match zero tests until the new files are in place; once
each stage lands, rerun the matching subset.

After each stage, run focussed validation:

```bash
ACTION=focused-1-3-5
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
cargo nextest run -p axinite skills::registry::tests 2>&1 | tee "$LOG"
cargo nextest run -p axinite skills::file_read 2>&1 | tee "$LOG"
cargo nextest run -p axinite tools::builtin::skill_tools 2>&1 | tee "$LOG"
cargo nextest run -p axinite --test channels skills_upload 2>&1 | tee "$LOG"
```

Run `coderabbit review --agent` after each major milestone:

```bash
ACTION=coderabbit-1-3-5
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
coderabbit review --agent 2>&1 | tee "$LOG"
```

Run final gates sequentially:

```bash
ACTION=check-fmt-1-3-5
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make check-fmt 2>&1 | tee "$LOG"

ACTION=lint-1-3-5
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make lint 2>&1 | tee "$LOG"

ACTION=test-1-3-5
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make test 2>&1 | tee "$LOG"
```

If repository policy requires the aggregate gate as well, run:

```bash
ACTION=all-1-3-5
LOG="/tmp/${ACTION}-axinite-$(git branch --show-current).out"
make all 2>&1 | tee "$LOG"
```

Run Markdown and whitespace checks after documentation edits:

```bash
bunx markdownlint-cli2 \
  docs/execplans/1-3-5-installation-and-runtime-tests-for-bundled-skills.md \
  docs/roadmap.md docs/users-guide.md docs/developers-guide.md \
  docs/agent-skills-support.md
git diff --check
```

Commit only after the gates pass, using the file-based commit-message workflow
from the `commit-message` skill. Push and refresh the draft pull request using
the `pr-creation` skill.

## Validation and acceptance

The milestone is accepted when all of the following are true:

- A round-trip test installs a bundle through
  `SkillInstallPayload::DownloadedBytes` and through
  `SkillInstallPayload::ArchiveBytes` and asserts that every archive entry is
  preserved on disk byte-for-byte, including `references/<nested>` and
  `assets/` entries.
- A `proptest` round-trip test generates arbitrary valid bundle
  manifests and asserts the same byte-equality after install over at least 32
  cases per run with shrinking enabled.
- A Linux-gated test reads every text entry of an installed bundle
  through `read_skill_file` and asserts inline `Success` content matches the
  installed bytes.
- A Linux-gated test asserts the typed `NonInlineAsset` payload
  for an installed PNG asset, with `metadata.size`, `metadata.mime_type`, and
  `metadata.fetch_hint` populated.
- A non-Linux-gated test asserts the documented `IoError`
  fallthrough for the same fixture.
- The multipart upload integration test base covers at least six
  malformed-archive categories from the bundle validator, each returning HTTP
  `400 Bad Request`, the documented `invalid_skill_bundle` body, and an install
  directory with no surviving entries.
- The multipart upload integration suite includes a single happy
  path that uploads, then drives `SkillReadFileTool` against each documented
  bundled path and asserts content and MIME type.
- One new `rstest-bdd` scenario in
  `src/agent/dispatcher/tests/features/active_skill_context.feature` names the
  progressive-disclosure contract for bundle skills; the matching step
  definitions live next to `SkillContextWorld`.
- The named regression test
  `test_install_preserves_references_and_assets_regression_rfc0003` asserts
  exact on-disk file-set equality against the input manifest, with a doc
  comment citing RFC 0003 and the dropped-files bug.
- The install→read journey is exercised through table-driven
  `#[rstest] #[case]` parameterization in
  `src/tools/builtin/skill_tools/tests.rs`, not through Gherkin.
- `docs/roadmap.md` marks `1.3.5` done, and
  `docs/agent-skills-support.md` plus `docs/developers-guide.md` describe the
  new test inventory and fixture pattern.
- `coderabbit review --agent` has no unresolved blocking concerns.
- `make check-fmt`, `make lint`, and `make test` succeed. If
  `make all` is run as the aggregate gate, it succeeds too.

Quality criteria:

- Tests: targeted Rust unit, integration, and behavioural tests
  pass, including the new round-trip property test.
- Lint and typecheck: clippy and formatting gates pass through the
  Makefile targets.
- Security: tests assert that no install transport leaves a staged
  directory on the failure path, and no read-path test exercises paths outside
  the loaded skill root.
- Documentation: changed Markdown passes markdownlint and
  `git diff --check`.

## Idempotence and recovery

All steps are additive. Re-running a stage with the same inputs produces the
same outcomes. If a stage's tests fail, inspect the corresponding
`/tmp/*-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`
log before rerunning. If a generated property-test case is hard to reproduce,
copy the failing seed from the log and rerun with
`PROPTEST_CASES=1 PROPTEST_REPLAY=<seed>` to confirm before fixing.

If the round-trip property test surfaces a real bug in the install pipeline
rather than a generator flaw, stop, log the finding in
`Surprises & Discoveries`, and ask whether the bug fix belongs in this branch
or in a separate change.

If the documentation gates flag any new Markdown lines longer than 80 columns
or unwrapped paragraphs, fix locally; do not weaken the markdownlint
configuration.

If implementation has to be rolled back before commit, use ordinary Git reverse
patches. Do not use `git reset --hard` or `checkout` on shared paths.

## Interfaces and dependencies

Test surfaces and the production seams they exercise:

```text
src/skills/test_support.rs
  fn installed_bundle_fixture(entries: &[(&str, &[u8])])
    -> InstalledBundleFixture
  // returns by-value; callers wrap Arc<RwLock<_>> locally if needed

src/skills/registry/tests/install.rs
  #[rstest]
  test_install_preserves_references_and_assets_regression_rfc0003
    // parameterised over DownloadedBytes and ArchiveBytes

src/skills/registry/tests/prop_tests.rs
  prop_bundle_round_trip_preserves_entries
    // ProptestConfig::with_cases(32).max_shrink_iters(64)

src/skills/file_read/install_tests.rs  // or extend tests.rs
  test_read_skill_file_after_install_returns_each_text_entry
  test_read_skill_file_after_install_returns_non_inline_metadata_for_binary
  test_read_skill_file_after_install_returns_io_error_on_non_linux

tests/channels/skills_upload.rs
  multipart_skill_bundle_upload_rejects_malformed_bundles
    // #[rstest] over MalformedKind enum
  multipart_skill_bundle_upload_rejects_non_skill_filename
  multipart_skill_bundle_upload_round_trip_reads_each_entry
    // happy path: upload then drive SkillReadFileTool

src/agent/dispatcher/tests/features/active_skill_context.feature
  Scenario: Activated bundle skill does not eagerly load ancillary files

src/agent/dispatcher/tests/skill_bundle_context_bdd.rs
  // step definitions for the progressive-disclosure scenario

src/tools/builtin/skill_tools/tests.rs
  test_skill_read_file_tool_after_install_returns_each_documented_entry
    // #[rstest] #[case] over the documented bundled path manifest
  test_skill_read_file_tool_after_install_returns_non_inline_for_png
```

Stable production entry points the tests must call (no new production
interfaces required):

```rust
crate::skills::registry::SkillRegistry::prepare_install_to_disk(
    install_root: &Path,
    payload: SkillInstallPayload,
) -> Result<PreparedSkillInstall, SkillRegistryError>;

crate::skills::registry::SkillRegistry::commit_install(
    prepared: PreparedSkillInstall,
) -> Result<(), CommitPreparedInstallError>;

crate::skills::file_read::read_skill_file(
    skill: &LoadedSkill,
    requested_path: &str,
) -> SkillReadFileResponse;

crate::tools::builtin::skill_tools::SkillReadFileTool::new(
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
) -> SkillReadFileTool;
```

No new runtime dependencies. New test-only dependencies must be recorded in
`Decision Log` with a one-line justification.

## Artefacts and notes

- `/tmp/focused-registry-stage-b-1-3-5-rerun-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `3 tests run: 3 passed`, covering
  `test_install_preserves_references_and_assets_regression_rfc0003` for
  `DownloadedBytes` and `ArchiveBytes`, plus
  `prop_bundle_round_trip_preserves_entries`.
- `/tmp/focused-file-read-stage-c-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `5 tests run: 5 passed`, covering installed-bundle reads for `SKILL.md`,
  nested references, text assets, and PNG `non_inline_asset` metadata on Linux.
- `/tmp/focused-tool-stage-e-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `5 tests run: 5 passed`, covering `SkillReadFileTool` after a real staged
  install.
- `/tmp/focused-bdd-stage-e-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `2 tests run: 2 passed`, covering the existing metadata BDD and the new
  progressive-disclosure BDD scenario.
- `/tmp/focused-upload-stage-d-1-3-5-rerun3-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `13 tests run: 13 passed`, covering malformed multipart uploads, filename
  rejection, the upload happy path, and upload-to-read on Linux.
- `/tmp/check-fmt-pre-coderabbit-1-3-5-rerun-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make check-fmt` passed after applying `cargo fmt`.
- `/tmp/lint-pre-coderabbit-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make lint` passed across the configured clippy matrix.
- `/tmp/test-pre-coderabbit-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make test` passed with `4215 tests run: 4215 passed` for the workspace
  nextest phase and `5 passed` for `tools-src/github/Cargo.toml`. The existing
  `schema_helpers_ui::ui` test was slow at 272.200 seconds.
- `/tmp/coderabbit-stage-abcde-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `coderabbit review --agent` completed with `findings: 0` for the code/test
  milestone.
- `/tmp/markdownlint-stage-f-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  markdownlint passed for the execplan, roadmap, user's guide, developer's
  guide, and agent-skills support document.
- `/tmp/diff-check-stage-f-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `git diff --check` passed after documentation updates.
- `/tmp/coderabbit-stage-f-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `coderabbit review --agent` completed with `findings: 0` for the
  documentation milestone.
- `/tmp/all-final-1-3-5-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make all` passed on the final tree, including formatting, clippy, `4215`
  workspace nextest tests, and `5` GitHub tool tests.
- `/tmp/check-fmt-rebase-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  `/tmp/test-rebase-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  `/tmp/typecheck-rebase-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  and
  `/tmp/lint-rebase-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  post-rebase formatting, test, typecheck, and lint gates passed.
- `/tmp/markdownlint-hook-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make markdownlint` passed after removing the duplicate blank line from
  `docs/developers-guide.md`.
- `/tmp/diff-check-hook-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `git diff --check` passed for the markdownlint cleanup.
- `/tmp/audit-postgres-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make audit` passed after refreshing the PostgreSQL crates in `Cargo.lock`.
- `/tmp/typecheck-postgres-audit-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make typecheck` passed after the audit fix.
- `/tmp/all-postgres-audit-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make all` passed after the audit fix.
- `/tmp/clippy-libsql-unused-code-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  the Windows-CI equivalent libSQL clippy profile passed on Linux:

  ```shell
  cargo clippy --all --benches --tests --examples --no-default-features --features libsql -- -D warnings
  ```

- `/tmp/check-fmt-unused-code-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  `/tmp/lint-unused-code-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  and
  `/tmp/test-unused-code-fix-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  formatting, lint, and test gates passed after the target-specific
  unused-code fix.
- `/tmp/check-fmt-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  `/tmp/typecheck-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  `/tmp/lint-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`,
  and
  `/tmp/test-axinite-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  review-feedback formatting, typecheck, lint, and test gates passed. The
  workspace nextest phase ran `4238` tests with `4238` passed; the GitHub tool
  test phase ran `5` tests with `5` passed.
- `/tmp/all-review-feedback-1-3-5-installation-and-runtime-tests-for-bundled-skills.out`:
  `make all` passed after the review-feedback consolidation.

## Revision note

2026-06-02: Created the initial draft for approval. The draft captures the gap
analysis against existing skill-bundle test coverage, the proposed round-trip
property and behavioural additions, the negative multipart matrix,
documentation sync, quality gates, and approval requirement for roadmap item
`1.3.5`.

2026-06-02: Folded in prior-art research findings: named the regression test
`test_install_preserves_references_and_assets_regression_rfc0003`, introduced a
`MalformedKind` enum for the gateway negative matrix, added a
progressive-disclosure BDD scenario asserting that ancillary files stay unread
after activation, recorded CVE references for traversal and symlink rejection,
and added an explicit atomicity observation under `Surprises & Discoveries`.

2026-06-02 (Logisphere expert review revisions): dropped the duplicate
`test_install_preserves_all_documented_entries` and
`test_uploaded_archive_preserves_all_documented_entries` cases in favour of one
parameterized regression test; resolved the property-test case-count
contradiction (32 cases with bounded shrinking); added a uniqueness invariant
to the proptest generator and switched to `CompressionMethod::Stored`;
relocated the progressive-disclosure scenario to the dispatcher BDD module;
demoted the install→read journey from a `.feature` scenario to
`#[rstest] #[case]` parameterization; renamed the non-Linux fallthrough test
for clarity; added Decision Log entries for the fixture ownership model, the
BDD/rstest split, and the scenario placement; recorded the RFC §3
collision-handling gap as a future-work observation; added a CodeRabbit-retry
tolerance.

2026-06-16: Marked the plan complete and recorded post-completion branch
hardening: no-conflict rebase onto `origin/main` at `8045e754`, markdownlint
cleanup, PostgreSQL RustSec lockfile refresh, and Windows/libSQL unused-code
lint fixes.

2026-06-21: Recorded review-feedback follow-up. Added the missing module-level
documentation for deterministic bundle install tests, documented the public
test fixture/archive-helper APIs in the developer guide, and consolidated
internal ZIP archive construction on `src/skills/test_support.rs`, including
the external upload integration test through the existing `test-helpers`
feature.
