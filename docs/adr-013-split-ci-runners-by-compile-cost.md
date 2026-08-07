# ADR-013 — Split CI runners by compile cost

**Status:** Accepted **Date:** 2026-08-05 **Deciders:** `@leynos`

## Context

Every Linux job in the repository ran on `ubicloud-standard-8`, an 8-vCPU paid
runner chosen because a full workspace build is the dominant CI cost. That
choice was applied uniformly rather than per job, so it also covered work that
never compiles anything: the two pull-request labelling workflows, the
regression-test check, the Claude Code review, and the weekly dependency
audit. Those jobs are single-threaded shell scripts, an action call, or an
API-bound agent run; the extra vCPUs sit idle.

A July 2026 Ubicloud usage audit attributed roughly 1,600 billed premium-8
minutes per month to those five workflows. Axinite is a public repository, so
GitHub-hosted `ubuntu-latest` runners execute the same work at no cost, and
the standard 2-vCPU hosted runner is not the bottleneck for any of them.

## Decision

Select the runner per job, from the job's compile cost:

- A job that compiles the workspace — `cargo build`, `cargo test`,
  `cargo nextest`, `cargo clippy`, `cargo llvm-cov`, `cargo component`, or the
  `make` targets that wrap them — runs on `ubicloud-standard-8`.
- A job that does not compile runs on GitHub-hosted `ubuntu-latest`.

The five non-compiling workflows moved accordingly. Windows jobs keep
`windows-latest` and the release workflow keeps its pinned `ubuntu-22.04`
images, both for reproducibility rather than cost.

The policy is enforced by `tests/workflow_contracts/runner_policy_test.py`,
which records the runner for every job in the repository. Adding a job, or
moving one between pools, fails until the recorded policy is updated
deliberately. The same suite asserts that no job on the free pool runs a
compile command or installs a Rust build cache.

## Rationale

Runner selection is a per-job property, not a per-repository one. Paying for
vCPUs a job cannot use is waste with no compensating benefit, and the
alternative — leaving everything on the paid pool because it is simpler —
costs roughly 1,600 billed minutes a month for jobs whose wall-clock time is
dominated by network round trips and process startup.

Encoding the rule as a contract test rather than a comment matters because the
failure mode is silent: a new job copied from an existing workflow inherits
whichever runner the template used, and nothing surfaces the mistake until the
next billing audit. A test that enumerates every job turns that into a
review-time question.

`release-plz.yml` is deliberately untouched. Its jobs are gated to the `nearai`
repository owner and never execute here, so moving them would change nothing
observable while diverging from upstream.

## Consequences

- Non-compile pull-request feedback moves to the free pool, so it competes for
  GitHub's shared hosted-runner concurrency rather than Ubicloud's. These jobs
  run on every pull-request event, so any queueing regression is visible
  immediately.
- Adding a workflow or a job now requires an edit to `RUNNER_POLICY` in
  `tests/workflow_contracts/runner_policy_test.py`. That is the intended
  friction: the runner choice becomes an explicit review decision.
- A free-runner job that later grows a build step fails its contract test
  rather than silently running a compile on a 2-vCPU machine.

## Alternatives considered

- **Leave everything on Ubicloud.** Simplest, and wrong: it keeps paying for
  capacity that five workflows demonstrably cannot use.
- **Move every Linux job to `ubuntu-latest`.** Free, but the workspace build
  is the reason the paid pool exists; hosted runners lack both the vCPUs and
  the disk headroom the coverage and end-to-end jobs need.
- **Document the split without a test.** Rejected for the silent-inheritance
  failure mode described above.
