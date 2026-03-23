# Add nightly mutation testing workflow with cargo-mutants

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: IMPLEMENTED

## Purpose / big picture

After this change, IronClaw runs `cargo-mutants` nightly against files changed
in the past 24 hours on the targeted branch. The workflow uploads a structured
log of surviving mutants as a downloadable GitHub Actions artefact. Developers
can also trigger the workflow manually on any branch, supplying an explicit list
of source paths to mutate.

The surviving-mutant log is a concrete, prioritized backlog: each entry names a
file, function, and mutation that no test caught, giving developers an immediate
target for a new or stronger assertion.

Observable success: after merging, navigating to Actions > Mutation Testing in
the GitHub UI shows a scheduled run (or a manual dispatch) whose artefact
`mutants-report` contains `mutants.out/caught.txt`, `mutants.out/unviable.txt`,
and `mutants.out/survived.txt`, and the job summary prints a human-readable
table of survivors.

## Constraints

- The workflow must not be a required status check. Surviving mutants are
  informational, not a merge gate.
- The workflow must not modify the existing CI pipeline or any required check.
- The workflow must use the same Rust toolchain, linker, and cache conventions
  as the existing `test.yml` and `coverage.yml` workflows (stable toolchain,
  mold linker, `Swatinem/rust-cache`, `taiki-e/install-action`).
- The workflow file must live at `.github/workflows/mutation-testing.yml`.
- All prose in the workflow file and ADR must follow `en-GB-oxendict` spelling.
- The `tools-src/github` crate (WASM target) is excluded from the workspace
  and uses a separate manifest. It must be excluded from the mutation run
  because `cargo-mutants` operates on workspace members.
- The nightly scheduled run must scope mutations to files changed in the past
  24 hours on the target branch, not the entire codebase, to keep compute time
  manageable.
- Manual dispatch must accept an explicit list of source file paths or globs
  to mutate, giving developers surgical control.

## Tolerances (exception triggers)

- Scope: the implementation must not touch more than 3 files (the workflow
  YAML, the ADR update, and the contents file if needed). If more files are
  required, stop and escalate.
- Interface: no existing workflow or Makefile target may be modified.
- Dependencies: no new runtime dependency on the Rust codebase. `cargo-mutants`
  is a CI-only tool installed via `taiki-e/install-action`.
- Iterations: if the workflow YAML does not pass `actionlint` (if available) or
  manual review within 2 attempts, stop and escalate.
- Ambiguity: if `cargo-mutants` does not support `--file` or `--re` filtering
  in the installed version, stop and document the gap.

## Risks

- Risk: `cargo-mutants` may not support file-path-based filtering granularly
  enough for the "changed in 24 hours" scoping.
  Severity: medium
  Likelihood: low
  Mitigation: `cargo-mutants` supports `--file <glob>` to restrict mutation
  to named source files. The workflow will compute the changed-file list via
  `git diff` and pass each file as a `--file` argument. If no files changed,
  the job exits early with success.

- Risk: nightly runs on a large diff may still be expensive.
  Severity: low
  Likelihood: medium
  Mitigation: the 24-hour window naturally bounds the diff. If a bulk merge
  lands, the run may be slow but will complete; GitHub Actions has a 6-hour
  job timeout by default, and we set a tighter `timeout-minutes`.

- Risk: flaky tests may cause inconsistent mutant verdicts.
  Severity: medium
  Likelihood: medium
  Mitigation: `cargo-mutants` re-runs the baseline test suite first and
  aborts if the baseline fails, surfacing flakiness early. This is an existing
  test-suite concern, not a workflow concern.

## Progress

- [x] Draft ExecPlan.
- [x] Obtain approval.
- [x] Create `.github/workflows/mutation-testing.yml`.
- [x] Update ADR 008 status from `Proposed` to `Accepted` and record the
      nightly + changed-files design decision.
- [x] Update `docs/contents.md` if the ADR is not already listed.
- [x] Validate the workflow YAML (syntax, act dry-run, or manual review).
- [x] Commit and gate (`make all`, `bunx markdownlint-cli2`).

## Surprises & discoveries

- Markdownlint (MD031) requires blank lines before and after fenced code
  blocks even when they appear inside list items. The execplan's embedded
  shell snippets needed blank lines added around them.
- The MD013 line-length rule applied to the Stage D validation command listed
  in the plan body. Rephrased to keep lines within 80 columns.
- During rebase onto `main`, `ac20fe4` ("Feature-gate Docker sandbox support
  cleanly") was found to have added a separate ADR 007 ("Stable capability
  probes must ignore ambient `RUSTC_WRAPPER`"). The mutation-testing ADR was
  renumbered to 008 to avoid a collision: the file was renamed, its heading
  updated, and all references corrected.

## Decision log

- Decision: scope nightly runs to files changed in 24 hours using
  `git log -m --since` and `cargo-mutants --file`.
  Rationale: a full-workspace mutation run would take hours. Scoping to recent
  changes makes nightly runs tractable while still surfacing regressions near
  the point of introduction.
  Date/Author: 2026-03-22 / plan author.

- Decision: manual dispatch accepts a newline-separated list of paths rather
  than a package list.
  Rationale: `--file` operates on source-file paths, which is more granular
  than `--package` and matches the mental model of "these files changed;
  mutate them." Developers can use globs (e.g. `src/agent/**/*.rs`).
  Date/Author: 2026-03-22 / plan author.

- Decision: set `timeout-minutes: 120` on the mutation job.
  Rationale: mutation testing is inherently slow. Two hours is generous for a
  scoped nightly run while still preventing runaway jobs. Full-workspace runs
  triggered manually may need longer, but 120 minutes is a reasonable starting
  point — the timeout can be raised once real-world durations are observed.
  Date/Author: 2026-03-22 / plan author.

- Decision: pass `-m` to `git log` when computing the changed-file list.
  Rationale: without `-m`, `git log --name-only` omits file paths from merge
  commits. A recently landed merge whose side-branch commits are older than 24
  hours would produce an empty file list and cause mutation testing to be
  silently skipped. `-m` expands each merge commit relative to its parents,
  ensuring that files introduced via such a merge are captured.
  Date/Author: 2026-03-22 / plan author.

## Outcomes & retrospective

Implementation completed on 2026-03-22. Three files were created or modified:

- `.github/workflows/mutation-testing.yml` — new workflow file.
- `docs/adr-008-mutation-testing-with-cargo-mutants.md` — status promoted to
  Accepted, workflow design updated, outstanding decisions resolved. Renumbered
  from 007 to 008 during rebase to avoid collision with the capability-probes
  ADR added concurrently on `main`.
- `docs/contents.md` — ADR 008 added to the ADR index.

`make all` (fmt, lint, 3300 tests) passed without change. No Rust source was
touched. Markdown linting passed after fixing blank-line-around-fences and
line-length violations in this execplan document.

## Context and orientation

The repository is a Rust workspace with a single member crate (`ironclaw`) at
the root. Several out-of-tree crates under `tools-src/` and `channels-src/`
are excluded from the workspace and built separately.

Existing CI workflows live in `.github/workflows/`. The patterns relevant to
this plan are:

- `test.yml`: PR and push gate. Uses `dtolnay/rust-toolchain@stable`,
  `rui314/setup-mold@v1`, `Swatinem/rust-cache@v2`,
  `taiki-e/install-action@cargo-nextest`. Runs `make build-github-tool-wasm`
  before tests.
- `coverage.yml`: push-to-main only. Uses `cargo-llvm-cov` installed via
  `taiki-e/install-action`. Uploads artefacts to Codecov.
- `staging-ci.yml`: hourly schedule with `workflow_dispatch`. Uses a
  `check-changes` job that computes a diff range via `git diff` to decide
  whether to proceed.

The new workflow follows the same toolchain and action version conventions.

`cargo-mutants` is a Rust mutation testing tool. It discovers functions in the
source, generates mutations (e.g. replacing a function body with a default
return), builds the mutated code, and runs the test suite. Outcomes per mutant
are: caught (a test failed, good), survived (no test failed, gap found),
unviable (the mutation did not compile), or timeout.

Key `cargo-mutants` flags used in this plan:

- `--file <path>`: restrict mutations to the named source file. Can be
  repeated.
- `--output <dir>`: write results to the named directory (default:
  `mutants.out/`).
- `--timeout-multiplier <N>`: scale the per-mutant test timeout relative to
  the baseline.
- `--features <features>`: pass feature flags to cargo test.

## Plan of work

The work is a single stage: create the workflow file, update the ADR, and
validate.

### Stage A: create the workflow

Create `.github/workflows/mutation-testing.yml` with the following structure:

**Triggers.** The workflow fires on:

1. `schedule` — nightly at 03:00 UTC (`cron: "0 3 * * *"`).
2. `workflow_dispatch` — manual, with two inputs:
   - `branch` (string, default `main`): the branch to check out and mutate.
   - `paths` (string, default empty): newline-separated list of source file
     paths or globs to mutate. When empty on manual dispatch, the workflow
     uses the same 24-hour diff logic as the scheduled run.

**Permissions.** `contents: read` only.

**Concurrency.** Group `mutation-testing-${{ github.ref }}` with
`cancel-in-progress: true` so a new nightly run cancels a still-running
previous one.

**Job: `mutate`.** Runs on `ubuntu-latest` with `timeout-minutes: 120`.

Steps:

1. **Checkout.** `actions/checkout@v6` with `fetch-depth: 0` (needed for
   `git diff` on the 24-hour window). For `workflow_dispatch`, check out the
   `inputs.branch` ref.

2. **Compute file list.** A shell step that decides which files to mutate:
   - If `inputs.paths` is non-empty (manual dispatch with explicit paths),
     use those paths directly.
   - Otherwise, compute the list of `.rs` files changed in the past 24 hours
     on the current branch:

     ```bash
     git log -m --since="24 hours ago" --diff-filter=ACMR --name-only \
       --pretty=format: HEAD | grep '\.rs$' | sort -u
     ```

   - After computing the list, filter out files under `tools-src/` and
     `channels-src/`: those crates use separate manifests and are not
     workspace members; `cargo-mutants` cannot target them.
   - If the resulting list is empty, print a message and exit the job
     successfully (no files to mutate).
   - Write the file list to `$GITHUB_OUTPUT` as a multiline output named
     `files`.

3. **Install Rust.** `dtolnay/rust-toolchain@stable` with
   `targets: wasm32-wasip2`.

4. **Install mold.** `rui314/setup-mold@v1`.

5. **Rust cache.** `Swatinem/rust-cache@v2` with key `mutation-testing`.

6. **Install cargo-mutants.** `taiki-e/install-action@cargo-mutants`.

7. **Install cargo-nextest.** `taiki-e/install-action@cargo-nextest`
   (cargo-mutants can use nextest as the test runner via `--test-tool nextest`,
   matching the project's standard test runner).

8. **Install cargo-component.** `taiki-e/install-action@v2` with
   `tool: cargo-component`.

9. **Build WASM prerequisites.** `make build-github-tool-wasm` followed by
   `./scripts/build-wasm-extensions.sh --channels`, matching the test workflow.

10. **Run cargo-mutants.** Build the `--file` arguments from the computed file
    list and execute:

    ```bash
    file_args=""
    while IFS= read -r f; do
      [[ -n "$f" ]] && file_args+=" --file $f"
    done <<< "$FILES"

    # shellcheck disable=SC2086
    cargo mutants --no-shuffle --test-tool nextest \
      --timeout-multiplier 3 \
      --features test-helpers \
      ${file_args}
    ```

    The step uses `continue-on-error: true` so surviving mutants do not fail
    the job. The exit code is captured for the summary step.

11. **Print surviving mutants summary.** Parse `mutants.out/survived.txt` and
    append a Markdown summary to `$GITHUB_STEP_SUMMARY`:

    ```bash
    echo "## Surviving mutants" >> "$GITHUB_STEP_SUMMARY"
    if [[ -s mutants.out/survived.txt ]]; then
      echo '```' >> "$GITHUB_STEP_SUMMARY"
      cat mutants.out/survived.txt >> "$GITHUB_STEP_SUMMARY"
      echo '```' >> "$GITHUB_STEP_SUMMARY"
    else
      echo "No surviving mutants." >> "$GITHUB_STEP_SUMMARY"
    fi
    ```

12. **Upload artefact.** `actions/upload-artifact@v4` uploading `mutants.out/`
    as `mutants-report` with `retention-days: 14`.

### Stage B: update the ADR

In `docs/adr-008-mutation-testing-with-cargo-mutants.md`:

- Change status from `Proposed` to `Accepted`.
- Refine the workflow design section to reflect the nightly + 24-hour diff
  scoping (replacing the earlier "weekly" language) and the manual dispatch
  path-list input.
- Resolve the outstanding decisions section: schedule is nightly at 03:00 UTC,
  initial scope is changed files, and the summary is posted to the job summary
  (not a GitHub issue).

### Stage C: update the documentation index

If ADR 008 is not yet listed in `docs/contents.md`, add it in the appropriate
ADR group.

### Stage D: validate and commit

1. Run `bunx markdownlint-cli2` on changed Markdown files and fix any
   violations.
2. Run `make all` to confirm no Rust source was inadvertently changed.
3. Commit all files in a single commit with a message describing the addition
   of the mutation testing workflow, ADR acceptance, and documentation update.

## Concrete steps

All commands are run from the repository root
(`/data/leynos/Projects/axinite.worktrees/mutation-testing`).

Stage A — create the workflow file:

```bash
# The file is created by the agent using the Write tool.
# Path: .github/workflows/mutation-testing.yml
```

Stage B — update the ADR:

```bash
# The file is edited by the agent using the Edit tool.
# Path: docs/adr-008-mutation-testing-with-cargo-mutants.md
```

Stage C — update the contents file (if needed):

```bash
# The file is edited by the agent using the Edit tool.
# Path: docs/contents.md
```

Stage D — validate:

```bash
bunx markdownlint-cli2 \
  docs/adr-008-mutation-testing-with-cargo-mutants.md \
  docs/execplans/mutation-testing.md
```

Expected: no violations (exit 0).

```bash
make all
```

Expected: `check-fmt`, `lint`, and `test` all pass. No Rust source was
changed, so this is a no-op confirmation.

```bash
git diff --check
```

Expected: no whitespace errors.

## Validation and acceptance

Quality criteria:

- The workflow file `.github/workflows/mutation-testing.yml` is valid YAML
  and follows the conventions of the existing workflows.
- The nightly trigger uses `cron: "0 3 * * *"`.
- The `workflow_dispatch` trigger accepts `branch` (string, default `main`)
  and `paths` (string, default empty) inputs.
- The job computes a changed-file list from
  `git log -m --since="24 hours ago"` when no explicit paths are given, and
  exits early when the list is empty.
- `cargo-mutants` is invoked with `--file` arguments restricting mutations to
  the computed file list.
- `mutants.out/` is uploaded as a 14-day artefact named `mutants-report`.
- A Markdown summary of surviving mutants is written to `$GITHUB_STEP_SUMMARY`.
- ADR 008 status is `Accepted` with the outstanding decisions resolved.
- `bunx markdownlint-cli2` reports no violations on changed Markdown files.
- `make all` passes.

Quality method:

- Manual review of the workflow YAML against the criteria above.
- `bunx markdownlint-cli2` on all changed `.md` files.
- `make all` confirming no Rust regressions.

## Idempotence and recovery

All steps are idempotent. The workflow file and ADR edits can be re-applied
without side effects. If the commit needs to be amended or redone, the standard
`git reset` and re-commit workflow applies.

## Artefacts and notes

The primary artefact is the workflow file. A representative skeleton of the
YAML is embedded in the plan of work above. The final file will be produced
by the Write tool during implementation.

## Interfaces and dependencies

No new Rust interfaces. The only new dependency is `cargo-mutants`, installed
at CI time via `taiki-e/install-action@cargo-mutants`. This is a build/test
tool with no effect on the compiled binary.
