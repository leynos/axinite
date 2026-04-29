# Assemble representative Stilyagi fixtures

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: DRAFT

## Purpose / big picture

Roadmap item `1.3.1` gives Stilyagi a shared validation corpus before the first
meaningful feature slice depends on ad hoc strings hidden inside individual
tests. After this change, maintainers should be able to add Markdown, Python
docstring, and Rust documentation-comment extraction tests by referring to a
small, documented fixture corpus that already covers normal input, suppression
directives, and malformed-input recovery cases.

The observable outcome is simple: the repository contains named fixture files
for every syntax surface promised by v1, the fixture set is indexed by a
machine-readable manifest or an equivalent typed loader, and tests prove that
all fixture files are present, UTF-8 readable, categorized, and usable by the
current extraction boundary. Later roadmap items can add golden intermediate
representation (IR), command-line interface (CLI) snapshots, and fix
round-trip helpers without inventing new input data.

This plan is a draft only. Do not implement it until a human reviewer approves
the plan.

## Source documents and skills

The implementer must start by reading these repository documents in the
Stilyagi checkout:

- `AGENTS.md` for local agent rules, branch rules, command gates, and commit
  expectations.
- `docs/roadmap.md`, especially item `1.3.1`.
- `docs/stilyagi-design.md`, especially sections `7.1`, `10`, `11`, and `12`.
- `docs/rfcs/0004-stilyagi-rule-testing-framework.md`.
- `docs/complexity-antipatterns-and-refactoring-strategies.md`.
- `docs/rust-testing-with-rstest-fixtures.md`.
- `docs/rust-doctest-dry-guide.md`.
- `docs/reliable-testing-in-rust-via-dependency-injection.md`.
- `docs/rstest-bdd-users-guide.md`.
- `docs/users-guide.md` and `docs/developers-guide.md`.

The plan author verified the Stilyagi roadmap, design, and RFC 0004 from the
GitHub branch `1-2-3-plan-makefile-ci-smoke` because those files are not
present in the current Axinite checkout. In the implementation checkout, prefer
local files over remote copies and record any drift in `Decision Log`.

Relevant Codex skills:

- Use `execplans` to keep this file self-contained and current.
- Use `leta` before source exploration or refactoring.
- Use `rust-router` before changing Rust code, then load the smaller Rust skill
  it recommends for the exact issue under review.
- Use `rust-errors` if fixture loaders introduce typed error cases.
- Use `rust-types-and-apis` if the fixture manifest becomes a public or
  crate-facing API.
- Use `rust-performance-and-layout` only if fixture loading or extraction tests
  add measurable overhead to the normal test path.
- Use `en-gb-oxendict-style` when editing documentation.
- Use `commit-message` for any commit created from this work.

## Repository orientation

Stilyagi is a Python-distributed prose linter with a Rust extraction extension.
Rust owns Markdown parsing, host-language docstring or documentation-comment
extraction, source maps, and IR construction. Python owns the CLI, config,
rule execution, diagnostics, fixes, plugin loading, and the future pytest
testing harness.

The design recommends this shape:

```plaintext
stilyagi/
├── python/
│   └── stilyagi/
├── crates/
│   ├── stilyagi-core/
│   ├── stilyagi-markdown/
│   ├── stilyagi-tree-sitter/
│   ├── stilyagi-extract/
│   ├── stilyagi-ir/
│   └── stilyagi-pyext/
├── tests/
│   ├── golden/
│   ├── integration/
│   ├── performance/
│   └── rulepacks/
└── docs/
```

Roadmap item `1.3.1` should add the input corpus and the smallest scaffolding
needed to keep that corpus healthy. Roadmap item `1.3.2` owns golden IR, CLI
snapshots, and fix round-trip helpers. Do not move those later helpers into
this slice unless the approved plan is revised.

## Constraints

- Keep implementation scoped to fixture assembly and corpus health checks. Do
  not implement golden IR assertions, CLI snapshot helpers, fix round-trip
  helpers, performance probes, or the full pytest plugin in this slice.
- Preserve the design's v1 syntax scope: Markdown, Python docstrings, and Rust
  documentation comments are stable; Markdown with JSX (MDX) remains preview
  or malformed-recovery input only.
- Keep fixtures as ordinary repository files, not generated strings hidden in
  tests.
- Keep malformed fixtures intentionally malformed. Tests may assert that they
  are readable and routed to recovery paths, but formatters must not "fix" the
  broken source.
- Use `rstest` for Rust unit and integration tests.
- Use `pytest` for Python tests.
- Add `rstest-bdd` and `pytest-bdd` behavioural coverage only where the
  repository already has, or can accept with narrow changes, a proportional BDD
  harness. If adding either BDD layer would create a large harness unrelated to
  fixture assembly, stop and escalate before continuing.
- Use dependency injection or explicit fixture paths. Do not rely on user-level
  config, persistent caches, global environment mutation, network access, or
  host-specific absolute paths.
- Documentation must use en-GB-oxendict spelling except for external APIs,
  commands, filenames, and code.
- Update `docs/roadmap.md` to mark item `1.3.1` done only after fixtures,
  tests, documentation, and validation have passed.
- Update `docs/users-guide.md` only for user-visible behaviour. If this slice
  only adds developer-facing fixture corpus material, record that no user-guide
  change was needed in `Decision Log`.
- Update `docs/developers-guide.md` with the fixture layout, how to add new
  fixture cases, and which later helpers should reuse the corpus.

## Tolerances

- Scope: if fixture assembly requires more than 18 changed files or more than
  900 net new lines excluding fixture content, stop and confirm that later
  roadmap work has not leaked into this item.
- Fixtures: if a single fixture file grows beyond 160 lines, split it unless a
  specific malformed-input case needs the full context.
- Manifest: if the manifest schema needs more than these fields, stop and
  review the design: `id`, `syntax`, `path`, `kind`, `expected_features`,
  `is_malformed`, and `notes`.
- Interface: if the current extraction bridge cannot consume one syntax family
  at all, do not widen the bridge silently. Add corpus loader tests and record
  the extraction gap for the later slice that owns that extractor.
- Dependencies: if new runtime dependencies are needed for fixture assembly,
  stop and escalate. Dev-only dependencies for `pytest-bdd` or equivalent BDD
  support require explicit approval if they are not already present.
- BDD: if BDD coverage requires more than one Rust feature file, more than one
  Python feature file, or a broad new test runner, stop and ask whether this
  roadmap item should be split.
- Validation: if `make check-fmt`, `make lint`, or `make test` cannot complete
  within the command timeout, split the gate into documented smaller targets
  and retain logs with `tee`.
- Repository mismatch: if implementation happens in a checkout that lacks the
  Stilyagi docs or mixed Python/Rust package layout, stop and ask for the
  correct repository before changing source code.

## Risks

- The current planning workspace is Axinite, not Stilyagi. The Stilyagi docs
  referenced by the request were verified remotely. Mitigation: implement only
  in a Stilyagi checkout that contains `docs/stilyagi-design.md` and
  `docs/rfcs/0004-stilyagi-rule-testing-framework.md`.
- Fixture scope can expand into golden IR, snapshot, and rule-test helper work.
  Mitigation: keep this item focused on shared input files, fixture indexing,
  and minimal corpus-health tests.
- Malformed fixtures can be over-normalized by formatters or editors.
  Mitigation: isolate malformed cases in clearly named files, document why they
  are broken, and exclude only specific fixture paths from formatters if the
  normal format gate cannot leave them intact.
- Python and Rust tests can accidentally assert different fixture contracts.
  Mitigation: use one fixture manifest or one documented directory convention
  read by both languages.
- Early extraction code may not yet expose full IR fields such as `line_index`,
  `segments`, owner metadata, and canonical JSON. Mitigation: this slice should
  not require those fields; it should name the fixtures so later slices can add
  golden assertions against them.
- Adding BDD libraries too early could create framework churn. Mitigation:
  write one narrow scenario per language only when the existing build spine
  supports it cleanly; otherwise stop for approval rather than inventing a
  parallel test framework.

## Fixture corpus shape

Create a fixture root such as `tests/fixtures/corpus/`. If the repository
already has a stronger convention, follow it and update this plan.

Use stable, descriptive filenames. A suitable first layout is:

```plaintext
tests/fixtures/corpus/
├── fixtures.toml
├── README.md
├── markdown/
│   ├── headings.md
│   ├── table-links.md
│   ├── suppressions.md
│   ├── malformed-link.md
│   └── malformed-table.md
├── python/
│   ├── module-class-function-docstrings.py
│   ├── decorators-and-nested-owners.py
│   ├── suppressions.py
│   └── malformed-incomplete-definition.py
└── rust/
    ├── module-and-item-doc-comments.rs
    ├── nested-items-and-impls.rs
    ├── suppressions.rs
    └── malformed-incomplete-item.rs
```

`fixtures.toml` should describe the corpus in a way both Rust and Python can
load. Do not put expected full IR payloads in this manifest. For this slice,
the manifest is only a durable index of what each fixture is meant to exercise.

Each syntax family must include happy-path and unhappy-path fixtures:

- Markdown: headings, lists if already cheap, tables, links, inline markup,
  code spans or blocks, suppression directives, and malformed recovery cases
  such as an unclosed link or damaged table.
- Python: module, class, function, and method docstrings; decorators; nested
  declarations; syntax-native suppressions; and a malformed file where the
  extractor should recover whatever docstring regions remain available.
- Rust: crate or module docs, item docs, function docs, impl-associated docs,
  syntax-native suppressions, and a malformed file where tree-sitter recovery
  can still expose partial documentation comments.

The README should explain how to add a new fixture, when to prefer extending an
existing fixture, and why expected IR belongs to roadmap item `1.3.2` rather
than this corpus item.

## Implementation plan

1. Confirm the working branch and repository identity.

   Run:

   ```plaintext
   git branch --show-current
   test -f docs/stilyagi-design.md
   test -f docs/rfcs/0004-stilyagi-rule-testing-framework.md
   ```

   If the branch is a main branch, stop and follow `AGENTS.md` branch guidance.
   If the Stilyagi docs are missing, stop and request the correct checkout.

2. Read the required documents and update this plan.

   Confirm the exact fixture root and test conventions from the local
   repository. If the local Makefile or test layout differs from this draft,
   update `Repository orientation`, `Fixture corpus shape`, and `Validation`
   before writing code.

3. Add the fixture corpus.

   Create the Markdown, Python, and Rust fixture files plus the manifest and
   README. Keep each fixture focused on one or two concepts. Put intentionally
   malformed input in files whose names begin with `malformed-` or otherwise
   make the broken state obvious.

4. Add shared fixture-loading helpers.

   Add the smallest Rust and Python helpers needed to discover the manifest,
   load fixture text, and filter by syntax or feature. The helpers should
   return typed errors rather than panicking on missing files or invalid
   manifest entries.

5. Add Rust tests with `rstest`.

   The Rust tests should prove that the manifest is valid, every declared file
   exists, all fixture text is UTF-8, fixture identifiers are unique, and every
   required syntax family has at least one happy-path and one malformed case.
   Where the current Rust extraction API exists, add smoke tests that feed the
   matching fixture files into that API and assert that malformed input is
   reported as recoverable rather than crashing.

6. Add Python tests with `pytest`.

   The Python tests should load the same manifest, assert the same corpus
   invariants, and exercise the public Python extraction wrapper where it
   already exists. Use `tmp_path` or injected fixture roots for any temporary
   project setup.

7. Add behavioural tests only within the approved tolerance.

   Prefer one Rust `rstest-bdd` scenario that states the corpus has complete
   syntax coverage and one Python `pytest-bdd` scenario that a temporary
   project can read representative fixtures. If the repository does not yet
   support either BDD layer without a broad harness, stop and ask for approval
   before adding dependencies or framework code.

8. Update documentation.

   Update `docs/developers-guide.md` with the fixture corpus layout, manifest
   fields, and rules for adding malformed cases. Update `docs/users-guide.md`
   only if the implementation changes user-visible commands or behaviour.
   Record any design decision in `docs/stilyagi-design.md`, most likely in the
   validation-plan section, if the fixture corpus layout becomes part of the
   project contract.

9. Mark the roadmap item done.

   After tests and docs are complete, change `docs/roadmap.md` item `1.3.1`
   from unchecked to checked and keep its success text accurate.

10. Validate and commit.

    Run formatting, linting, and tests sequentially with logs retained under
    `/tmp`, then run Markdown linting for changed docs and `git diff --check`.
    Commit only after the gates pass.

## Validation

Use Makefile targets where available. Do not run formatting, linting, and tests
in parallel.

Recommended command sequence:

```plaintext
BRANCH=$(git branch --show-current)
make check-fmt 2>&1 | tee "/tmp/check-fmt-stilyagi-${BRANCH##*/}.out"
make lint 2>&1 | tee "/tmp/lint-stilyagi-${BRANCH##*/}.out"
make test 2>&1 | tee "/tmp/test-stilyagi-${BRANCH##*/}.out"
bunx markdownlint-cli2 docs/execplans/1-3-1-assemble-representative-fixtures.md \
  docs/developers-guide.md docs/users-guide.md docs/stilyagi-design.md \
  docs/roadmap.md
git diff --check
```

If `make test` wraps both Rust and Python tests, that is the required broad
gate. If it does not, add the repository's canonical Rust and Python commands
from the local Makefile or developer guide, such as `cargo test` and
`python -m pytest`, and record the exact commands in this plan before running
them.

Expected success evidence:

```plaintext
make check-fmt exits 0
make lint exits 0
make test exits 0
markdownlint-cli2 reports no errors for changed Markdown files
git diff --check exits 0
```

## Progress

- [x] 2026-04-29T08:10:04Z: Drafted the approval-gated plan after checking the
  local branch and repository contents.
- [x] 2026-04-29T08:10:04Z: Used a Wyvern planning agent to scan the local
  documentation context and identify the Axinite versus Stilyagi repository
  mismatch.
- [x] 2026-04-29T08:10:04Z: Verified the referenced Stilyagi roadmap, design,
  and RFC 0004 from the GitHub branch because the local checkout does not
  contain those files.
- [ ] Await human approval before implementation begins.
- [ ] Confirm the implementation checkout is Stilyagi, not Axinite.
- [ ] Add the fixture corpus and manifest.
- [ ] Add Rust and Python corpus-health tests.
- [ ] Add proportional BDD coverage or escalate if the harness is out of scope.
- [ ] Update developer, user, design, and roadmap documentation as required.
- [ ] Run validation gates and commit the approved implementation.

## Surprises & Discoveries

- The current working tree is an Axinite checkout on branch
  `feat/fixture-plan-title`, while the requested roadmap item and design
  documents are for Stilyagi.
- The local `docs/roadmap.md` contains Axinite item `1.3.1` about `.skill`
  archive validation, not Stilyagi item `1.3.1` about representative fixtures.
- `docs/stilyagi-design.md` and
  `docs/rfcs/0004-stilyagi-rule-testing-framework.md` are absent locally but
  available in the referenced Stilyagi GitHub branch.
- The requested work is a plan, not implementation. The approval gate remains
  active.

## Decision Log

- Decision: Treat this plan as targeting the Stilyagi repository, despite being
  written in the current Axinite checkout.
  Rationale: the user explicitly cited Stilyagi documents, Stilyagi roadmap
  text, and a Stilyagi-specific output filename.

- Decision: Keep `1.3.1` limited to representative input fixtures and corpus
  health checks.
  Rationale: the roadmap assigns golden IR, CLI snapshots, and fix round-trip
  helpers to `1.3.2`, and moving them earlier would blur the delivery boundary.

- Decision: Use one manifest or equivalent shared loader as the corpus index.
  Rationale: Rust and Python tests need to agree on what the shared fixtures
  mean without duplicating fixture lists in two languages.

- Decision: Do not require full IR assertions in this slice.
  Rationale: the Stilyagi design says the current bridge may expose only
  `syntax` plus `regions[{kind, text}]`; full IR fields are part of later work.

- Decision: Escalate before adding broad BDD harness infrastructure.
  Rationale: the user requested behavioural tests, but the plan must keep
  framework work proportional to a fixture-assembly slice.

## Outcomes & Retrospective

No implementation has been performed. This section must be completed after the
approved plan is executed. Record the final fixture layout, test commands,
documentation updates, roadmap status, and any follow-up work left for roadmap
item `1.3.2`.
