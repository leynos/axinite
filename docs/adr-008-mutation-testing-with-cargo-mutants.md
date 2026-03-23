<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 008: Mutation testing with cargo-mutants

## Status

Accepted.

## Date

2026-03-22.

## Context and problem statement

IronClaw maintains line-coverage reports via `cargo-llvm-cov` and Codecov, but
line coverage only confirms that code is executed, not that tests actually
detect faults in it. A function body could be entirely replaced with a default
return value, and if no test fails, the existing suite provides a false sense of
safety for that code path.

Mutation testing addresses this gap by systematically injecting small faults
(mutants) into the source and checking whether the test suite catches each one.
Surviving mutants — mutations that no test detects — are concrete, actionable
pointers to missing or weak assertions.

The project needs a low-friction way to run mutation testing on a schedule,
record surviving mutants for triage, and allow maintainers to trigger the
analysis manually on any branch for validation.

## Decision drivers

- **Actionability over vanity metrics.** The output must be a list of surviving
  mutants that developers can turn directly into new or improved tests.
- **Non-blocking adoption.** Mutation testing is computationally expensive.
  It must not gate pull requests or slow down the existing continuous
  integration (CI) pipeline.
- **Manual trigger for peace of mind.** Maintainers should be able to run
  the workflow on an arbitrary branch before a release or after a large merge.
- **Rust ecosystem alignment.** The tool must work reliably with the project's
  workspace layout, feature flags, and minimum supported Rust version.

## Options considered

### Option A: cargo-mutants

`cargo-mutants` is purpose-built for Rust. It discovers mutation sites from the
abstract syntax tree (AST), runs the test suite for each mutant in an isolated
build directory, and produces structured output (JSON and plain text) listing
outcomes per mutant. It supports `--package`, `--exclude`, and `--features`
flags that align with the project's workspace and feature-flag conventions.

### Option B: Mutagen

Mutagen instruments code at compile time via a procedural macro. It requires
annotating source files with `#[mutate]` attributes, which introduces
maintenance overhead and couples production code to a testing tool.

### Option C: Universal mutation frameworks (e.g. mutmut, Stryker)

These tools target Python and JavaScript/TypeScript, respectively. Adapting them
to a Rust workspace would require custom integration work with no upstream
support.

| Topic                 | cargo-mutants        | Mutagen            | Universal frameworks |
| --------------------- | -------------------- | ------------------ | -------------------- |
| Language support      | Rust (native)        | Rust (proc-macro)  | Python / JS / TS     |
| Source annotation     | None required        | `#[mutate]` needed | N/A for Rust         |
| Structured output     | JSON + text          | Limited            | Varies               |
| Workspace support     | Yes                  | Partial            | No                   |
| Active maintenance    | Yes                  | Sporadic           | N/A for Rust         |

_Table 1: Comparison of mutation testing options._

## Decision outcome / proposed direction

Adopt **cargo-mutants** as the mutation testing tool, run it as a scheduled
GitHub Actions workflow with a manual dispatch option, and upload surviving
mutant reports as workflow artefacts.

### Workflow design

1. **Trigger.** The workflow runs nightly at 03:00 UTC
   (`cron: "0 3 * * *"`) and supports `workflow_dispatch` with two inputs:
   `branch` (string, default `main`) and `paths` (string, default empty).
2. **Scoping.** When `paths` is empty, the job computes the list of `.rs`
   files changed in the past 24 hours via
   `git log -m --since="24 hours ago" --diff-filter=ACMR --name-only`. The
   `-m` flag ensures merge commits expand their file lists relative to each
   parent, so a recently landed merge that brings in older side-branch commits
   is not silently skipped. Files under `tools-src/` and `channels-src/` are
   then excluded because those crates use separate manifests and are not
   workspace members; `cargo-mutants` operates on workspace members only. Each
   remaining file is passed via `--file`. If no files remain, the job exits
   early with success. When `paths` is non-empty (manual dispatch), those
   paths are used directly, after the same non-workspace exclusion filter.
3. **Execution.** The job installs `cargo-mutants` via `taiki-e/install-action`
   and runs `cargo mutants --test-tool nextest --features test-helpers` with
   the computed `--file` arguments. The step uses `continue-on-error: true`
   so surviving mutants do not fail the job.
4. **Output.** `cargo-mutants` writes results to `mutants.out/`. The workflow
   uploads the full output directory as a GitHub Actions artefact named
   `mutants-report` with a 14-day retention period.
5. **Surviving mutant summary.** After the mutation run, the contents of
   `mutants.out/survived.txt` are appended to `$GITHUB_STEP_SUMMARY` as a
   fenced code block, visible in the Actions UI without downloading the
   artefact.
6. **Non-blocking.** The workflow is not a required status check. A surviving
   mutant is informational, not a gate failure.
7. **Timeout.** The mutation job has a 120-minute timeout — generous for a
   scoped nightly run while preventing runaway jobs. The timeout can be raised
   once real-world durations are observed.

### Scoping rationale

Restricting nightly runs to files changed in the past 24 hours keeps
wall-clock time tractable: a full-workspace mutation run would take hours,
whereas a diff-scoped run surfaces regressions near the point of introduction.
Manual dispatch allows wider or narrower scope on demand.

## Goals and non-goals

- Goals:
  - Surface surviving mutants as an actionable backlog for test improvement.
  - Provide a manually triggerable workflow for pre-release or post-merge
    validation on any branch.
  - Keep mutation testing decoupled from the pull-request CI gate.
- Non-goals:
  - Achieving zero surviving mutants. Some mutants are semantically equivalent
    or affect code paths that are intentionally untested.
  - Replacing line-coverage reporting. Mutation testing complements coverage;
    it does not supersede it.
  - Running mutation analysis on every pull request.

## Known risks and limitations

- **Compute cost.** Mutation testing rebuilds and re-tests the project for
  every mutant. A full workspace run may take hours. The scheduled cadence and
  optional package scoping mitigate this.
- **Flaky tests.** A test that fails intermittently may cause a mutant to
  appear caught or missed inconsistently. The existing test suite should be
  stabilized independently.
- **Equivalent mutants.** Some mutations produce semantically identical
  behaviour (e.g. replacing `>=` with `>` when the boundary value is never
  reached). These will appear as survivors and must be triaged manually.

## Resolved decisions

- **Schedule.** Nightly at 03:00 UTC (`cron: "0 3 * * *"`).
- **Initial scope.** Files changed in the past 24 hours on the target branch,
  computed via `git log -m --since="24 hours ago"`. Widening the scope is
  straightforward once triage cadence is understood.
- **Summary destination.** Surviving mutants are posted to the GitHub Actions
  job summary (`$GITHUB_STEP_SUMMARY`) for immediate visibility, supplemented
  by the `mutants-report` artefact for full detail. No automatic issue creation
  is implemented in the initial version.
