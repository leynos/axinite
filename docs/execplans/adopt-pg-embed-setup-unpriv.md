# Give the test harness an embedded PostgreSQL

This ExecPlan (execution plan) is a living document. The sections `Constraints`,
`Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`, `Decision Log`,
and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: NOT STARTED

## Purpose / big picture

After this work, every Linux lane that runs the workspace suite has a
PostgreSQL to run it against, and no workflow declares a service container to
provide one. A developer on Linux or macOS with no local PostgreSQL gets one
too, from the same code path, so the outcome a contributor sees is that the
fifty-one PostgreSQL-backed tests stop returning early and start asserting.

The scope is Unix, deliberately. No Windows lane runs the workspace suite today:
`test.yml`'s `windows-build` job only runs `cargo check`. The teardown and the
cross-process lock below are Unix-only, so no embedded cluster is bootstrapped
on Windows, and a Windows developer keeps today's skip. Extending the suite to
Windows is a separate plan with its own lifecycle, cleanup and acceptance
criteria.

Success is observable in four ways. First, `try_test_pg_db` in
`src/testing/postgres.rs` returns a live backend on a checkout with no database
configured and no service running, and the forty PostgreSQL-backed tests that
fail on a host whose PostgreSQL rejects the local user pass there with no host
database involved. Second, `test.yml`'s `tests` job exports
`AXINITE_REQUIRE_POSTGRES`, so an unreachable database there fails the lane
rather than skipping it, which closes issue `#374`. Third, `coverage.yml`
declares no `services:` block and applies no migrations with `psql`. Fourth,
the per-lane wall-clock cost of all of this is measured and recorded, warm and
cold, rather than estimated.

This plan exists because the adoption is larger than it looks and was deferred
once already. It records what has to be built and why none of it is optional,
so the work proceeds deliberately rather than being rediscovered.

## Approval gates

- Dependency available
  Acceptance criteria: `pg-embed-setup-unpriv` 0.6.0 is published with its
  prebuilt-extension hook, and the pgvector archives it installs are released.
  The migrations run `CREATE EXTENSION vector` (`migrations/V1__initial.sql`),
  and 0.5.2 cannot supply the extension. Sign-off: the maintainer of that crate
  reports the release. The alternative, guarding the extension out of the test
  migrations on 0.5.2, changes what the tests prove and is the user's decision,
  not the implementer's.
- Plan approved
  Acceptance criteria: the build items below are agreed as necessary (the first
  now supplied by the library), the cost section is accepted as the basis for
  the decision, and the interim service container on `test.yml` is understood
  to be superseded rather than extended. Sign-off: human reviewer approves the
  ExecPlan before any dependency is added.
- Implementation complete
  Acceptance criteria: the harness bootstraps its own cluster, the ten call
  sites are unchanged or changed together, and both the service block and the
  `psql` migration step are gone. Sign-off: implementer marks the slice
  complete before final validation.
- Validation passed
  Acceptance criteria: the repository gates pass, the fifty-one tests are
  observed connecting rather than skipping, and the measured cold and warm
  per-lane costs are recorded here. Sign-off: implementer records the evidence
  immediately before push.
- Docs synced
  Acceptance criteria: the developers' guide, this plan and the workflow
  contracts describe the same arrangement. Sign-off: implementer completes the
  sync as the final pre-commit checkpoint.

## Repository orientation

Two facts make this adoption substantially cheaper here than in the reference
implementation, and they are the reason it is worth doing rather than living
with a service container.

- **Every PostgreSQL test reaches the database through one door.**
  `try_test_pg_db` in `src/testing/postgres.rs` is called from ten modules:
  `src/db/postgres/sandbox/tests/behavioural.rs`,
  `src/db/postgres/workspace.rs`, `src/history/store/actions/tests.rs`,
  `src/history/store/conversations/metadata.rs`,
  `src/history/store/conversations/singletons.rs`,
  `src/history/store/jobs/tests.rs`, `src/history/store/llm_calls.rs`,
  `src/history/store/routines/tests.rs`, `src/history/store/sandbox/events.rs`
  and `src/history/store/tools.rs`. Each takes an `Option<PgBackend>` and
  returns early on `None`. Nothing else connects.
- **Migrations already run from Rust.** `PgBackend::run_migrations` in
  `src/db/postgres/mod.rs` applies the seventeen files in `migrations/` through
  refinery, with the repair logic in `src/history/migrations.rs`. The reference
  implementation had to build a migration-hash template mechanism because it
  had no such path; this repository does not.

The CI side already has the pieces below in place. This plan depends on pull
request `#375` (fail rather than skip when a lane promised a database), which
introduces `AXINITE_REQUIRE_POSTGRES`; the items marked below arrive with it.

- `coverage.yml`'s `coverage` job declares the `pgvector/pgvector:pg16`
  service, applies migrations with `psql`, and exports `TEST_DATABASE_URL` and
  `DATABASE_URL` from one step guarded by `matrix.has_postgres`. With `#375`,
  the same step also exports `AXINITE_REQUIRE_POSTGRES`. `coverage.yml` runs on
  pushes to `main` and on dispatch, not on pull requests.
- The pull-request lane, `test.yml`'s `tests` job, exports none of the three.
  Exporting `AXINITE_REQUIRE_POSTGRES` there is the second success criterion
  above, and the risks below say why it waits for a proven bootstrap.
- `tests/workflow_contracts/coverage_database_test.py` asserts the coverage
  shape in both directions: that a leg's `has_postgres` matches whether its
  flags compile the `postgres` feature and, with `#375`, that the URL step also
  exports the requirement and that exactly one step does so. Those assertions
  describe the service arrangement and will be rewritten by this work, not
  merely extended.
- `.config/nextest.toml` declares the `default` and `ci` profiles and no test
  groups.

## Constraints

- Keep the harness's public shape. `try_test_pg_db` returning
  `Option<PgBackend>` is what the ten call sites expect; if a per-test database
  guard has to outlive the call, every call site changes in the same commit and
  the signature change is the subject of its own review, not a side effect.
- No `std::env::set_var`. The environment is process-wide and is unsound to
  mutate once a Tokio runtime exists, and every affected test is
  `#[tokio::test]`. Values that must be stable across test binaries are set in
  the `Makefile` and in the workflow `env:` block.
- No global single-threading. The workspace runs 4275 tests; suppressing
  parallelism across all of them to serialize fifty-one is not a trade this
  repository can make while CI minutes are the stated priority. Scope it with a
  nextest test group.
- The embedded cluster is a test-time concern. Nothing in the shipped binary
  may depend on it; the dependency is optional and enabled with the existing
  `test-helpers` feature alongside `postgres`.
- No test reaches a PostgreSQL the harness did not provision (user ruling,
  2026-09-23; see the decision log). The fallback to
  `postgresql://localhost/axinite_test` in `test_pg_db` is removed, not kept as
  a second path: it is how the suite reaches a host database today, and on a
  host whose PostgreSQL rejects the local user every PostgreSQL-backed test
  fails instead of running.
- `TEST_DATABASE_URL` survives only as long as the interim service container,
  and the two are retired together in the last milestone. While both exist, a
  set `TEST_DATABASE_URL` takes precedence and the embedded cluster is not
  bootstrapped, and an unreachable configured database is never silently
  replaced by the embedded one: that would test a different database from the
  one the lane asked for and hide the misconfiguration. With
  `AXINITE_REQUIRE_POSTGRES` set it is a failure; unset, it is a skip.
- Keep `AXINITE_REQUIRE_POSTGRES` meaning what `#375` defines: any value, even
  one the platform cannot render as Unicode, makes an unreachable database a
  failure rather than a skip.

## Tolerances (exception triggers)

- Scope: if the harness change needs more than 400 net lines before tests, or
  touches more than the ten call sites and the two workflows, stop and reassess.
- Parallelism: if the fifty-one tests cannot be made to pass under a scoped
  nextest group and require global single-threading, stop. That is a different
  decision and belongs to whoever owns the CI-minutes budget.
- Cost: if the measured warm per-lane cost exceeds the 23 seconds the service
  container was measured at in issue `#374`, stop and report. The adoption is
  then buying isolation and developer experience rather than time, which is
  still defensible but is no longer the same proposition.
- pgvector: if the 0.6.0 extension hook cannot provide the `vector` extension
  the migrations create, stop. Guarding the extension out is not a fallback the
  implementer may take; see the approval gates.
- Cold-start: if a cold binary download cannot be kept out of the test process,
  stop. A download inside the bootstrap is the failure mode that poisons a
  whole test binary; see the risks below.

## The build items

### 1. Cluster teardown at process exit

`shared_cluster_handle()` intentionally leaks its cluster guard so the cluster
outlives the caller, which is correct for a single test binary and wrong under
nextest, where each binary is a separate process. A server still running holds
the data directory, and the next binary cannot bootstrap on it.

The library now does this itself. `ClusterHandle::register_shutdown_on_exit()`
(in 0.5.2 and later) registers a process-exit reaper that terminates the
postmaster tree, and `test_support::shared_cluster_handle()` calls it before
leaking the guard, so each nextest process reaps its own cluster at exit.
Axinite uses the shared handle and writes no `atexit` handler of its own. The
earlier draft of this item planned one, modelled on the reference
implementation, with the process identity re-checked before each signal because
a PID can be reused during the wait. If the library's reaper turns out to lack
that check where it matters, the fix belongs upstream rather than in a second
reaper here. Axinite runs the library test binary plus several integration
binaries, and a second binary bootstrapping after the first has exited is the
acceptance check (see the verification plan). Windows is outside the scope
stated in the purpose.

### 2. Bootstrap retry outside the cached failure

The library stores its bootstrap result in a `OnceLock`. Once a failure is
recorded, every later call in that process returns the same cached error
immediately, and a retry loop around the call does not re-attempt anything. One
cold download failure therefore fails every PostgreSQL test in the binary, not
the one that raced.

This is still true on the library's main branch, and no retry-capable or
cache-warming API exists or is planned upstream. This is why a cache-warming
step is load-bearing rather than an optimization: the binaries must already be
present before any test process starts. The step runs the library's setup-only
mode once, before nextest, so the download happens outside the test clock. The
step needs a lock, because several test binaries may start together, and it
must resolve the release from a pinned URL rather than a floating one.

### 3. A scoped nextest test group

The cluster is shared and the databases are per test, so the tests are not
independent of each other's setup. The group caps concurrency for the
PostgreSQL-backed modules only, with a raised `slow-timeout`, leaving the other
4224 tests parallel. The reference implementation's group timeout has gone from
sixty seconds to a hundred and twenty to three hundred over two adoptions,
which is the number to expect rather than the number to start from.

### 4. CI cache paths and the worker binary

Two cache roots, kept out of the Cargo registry archive so that a `Cargo.lock`
change does not evict the PostgreSQL binaries and force a download during an
unrelated test change. The privilege-demotion worker is installed from a
checksum-verified release rather than compiled. Both working jobs need the
stable password and releases URL in their `env:` block, because they cannot be
set from inside the process.

## Cost

This section is deliberately unsatisfying, and the reason is worth stating.

**No cold-versus-warm figure exists in the reference implementation.** Neither
of its two adoption plans measured one. What is recorded upstream is a per-test
overhead of ten to fifty milliseconds for the shared-cluster, template-clone
pattern this plan would use, against a one-off bootstrap whose duration is not
recorded anywhere. The only wall-clock evidence is indirect: the timeout
budgets have ratcheted upward across two adoptions, and its coverage job
carries a 5400-second cargo wait whose comment attributes the need to the
serialized embedded-PostgreSQL suite.

**The overhead lands on every lane, not one.** The service container costs
roughly 23 seconds on the single leg that declares it, measured in issue
`#374`. The embedded cluster costs its bootstrap on every lane that runs the
workspace suite, which is the point of the change, and the honest comparison is
therefore not 23 seconds against some smaller number. It is 23 seconds on one
leg against a smaller number on several, plus the fifty-one tests actually
running everywhere instead of nowhere.

Measuring this is part of the work rather than a precondition for it: the first
green run records cold and warm per-lane figures here, and the tolerance above
says what to do if they come in above the service container's.

## Verification plan

- The fifty-one tests are observed connecting, not skipping. A run where they
  all skip is indistinguishable from success in the current arrangement, so the
  count of tests that actually reached a database is asserted, not inferred.
- `AXINITE_REQUIRE_POSTGRES` is exported on the pull-request lane, so a broken
  bootstrap fails rather than reverts to a silent skip. The existing wiring
  assertion in `src/testing/postgres/tests.rs` already proves the environment
  reaches the decision.
- A second test binary bootstraps successfully after the first has exited,
  which is the teardown item's acceptance check and cannot be observed from a
  single binary.
- A cold cache is exercised deliberately once, to measure it and to confirm the
  warming step covers it, rather than being discovered on an unrelated branch.

## Risks

- **A half-done adoption is worse than none.** Today an absent database is a
  skip. With the requirement exported and the bootstrap unreliable, it is a
  failed lane. The requirement must go on the pull-request lane only once the
  bootstrap is proven.
- **The cached failure turns one flake into a binary-wide failure.** See build
  item 2. This is the single largest operational risk and the reason the
  warming step is not optional.
- **The contract rewrite is a rewrite.** The 268 lines in
  `coverage_database_test.py` describe the service arrangement faithfully.
  Extending them to describe an absent service is not an edit; the assertions
  change direction.
- **Platform coverage is Unix only.** Teardown and the cross-process lock are
  Unix-only, so the embedded cluster is too; see the purpose. A Windows lane
  that later ran the suite would need its own lifecycle and cleanup, which is
  untested territory in the reference implementation too.

## Progress

Not started. This plan was written while deferring the adoption, so that the
reasons are recorded rather than rediscovered.

## Surprises & discoveries

- The reference implementation's own developers' guide documents a
  `pg_embed::shared_cluster()` helper and a CLI invocation that start a server;
  neither exists, the helper was renamed, and the CLI explicitly does not start
  one. Treat its prose as a starting point and its workflow files as the truth.
- The library and the worker binary are pinned to different versions there, on
  purpose: checksum-verified release archives only exist from the later one, so
  the worker is downloaded while the library stays where it is.
- 2026-09-23, from the crate's maintainer: 0.5.2 is the newest release and
  cannot supply pgvector, which the migrations require, so the adoption waits
  on 0.6.0. The worker binary matters only for root runs; on an unprivileged
  runner none is needed. Its release carries a `.sha256` sidecar that
  `cargo binstall` does not read, so a checksum-verified install is a separate
  step. The exit reaper that build item 1 planned is already in the library.

## Decision log

- 2026-09-23, user ruling: database tests do not use the host PostgreSQL at
  all; `pg-embed-setup-unpriv` exists for that. The forty tests that fail
  locally on a missing role are therefore a defect in how the tests reach a
  database, not a host problem, and the localhost fallback goes rather than
  surviving beside the embedded cluster.

- The interim service container on `test.yml` is the stopgap, not the design.
  It closes what issue `#374` reports, which is that no pull-request lane
  exercises PostgreSQL at all, and it is expected to be removed by this plan.
- Migrations stay in Rust. The `psql` step exists only because the service
  container needs one; `PgBackend::run_migrations` is the path this plan uses.

## Outcomes & retrospective

Not started.
