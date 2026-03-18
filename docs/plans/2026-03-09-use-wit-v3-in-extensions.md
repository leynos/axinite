# Align All Extensions And Channels To WIT 0.3.0

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as
work proceeds.

Status: COMPLETE

## Purpose / big picture

The target WebAssembly Interface Types (WIT) version for this work is
`0.3.0`, not `3.0.0`.

IronClaw already publishes both WebAssembly (WASM) host contracts as
`package near:agent@0.3.0;` in `wit/tool.wit` and `wit/channel.wit`, and the
curated in-tree extension matrix already declares `wit_version: "0.3.0"` in
its sidecar capabilities files and registry manifests. This means the task is
not a host-interface major-version migration. It is an audit-and-alignment pass
to prove every curated extension and channel is actually consistent with the
current `0.3.0` host contract, and to remove stale fallback paths that still
report older versions such as `0.1.0`.

Success is observable in four ways. First, the host WIT package lines and host
constants remain `0.3.0`. Second, every curated in-tree tool and channel builds
and instantiates against the current host WIT using the existing matrix checks.
Third, runtime reporting, persistence defaults, and upgrade paths tell the
truth about the current host contract instead of surfacing `0.1.0` fallbacks
for newly created records or compatibility messages. Fourth, contributor
documentation clearly states that new or rebuilt extensions must target WIT
`0.3.0`.

## Repository orientation

There are two canonical WIT entrypoints in this repo:

- `wit/tool.wit` for tools
- `wit/channel.wit` for channels

Every in-tree WASM extension points its `wit_bindgen::generate!` call directly
at one of those files via `../../wit/tool.wit` or `../../wit/channel.wit`, so
the full curated extension matrix rebuilds against the host contract in this
repository.

The in-tree extension source set currently consists of ten tools and four
channels.

- Tools: `github`, `gmail`, `google-calendar`, `google-docs`, `google-drive`,
  `google-sheets`, `google-slides`, `slack`, `telegram`, `web-search`
- Channels: `discord`, `slack`, `telegram`, `whatsapp`

The host-side compatibility and reporting surfaces live in:

- `src/tools/wasm/mod.rs` for `WIT_TOOL_VERSION` and `WIT_CHANNEL_VERSION`
- `src/tools/wasm/loader.rs` for WIT compatibility checks
- `src/channels/wasm/loader.rs` for channel-side WIT enforcement
- `src/tools/wasm/wrapper.rs` and `src/channels/wasm/wrapper.rs` for
  instantiation error messages
- `src/extensions/manager.rs` and `src/tools/builtin/extension_tools.rs` for
  upgrade and info flows

Published metadata and release verification live in:

- `tools-src/*/*-tool.capabilities.json`
- `channels-src/*/*.capabilities.json`
- `registry/tools/*.json`
- `registry/channels/*.json`
- `scripts/build-wasm-extensions.sh`
- `scripts/check-version-bumps.sh`
- `.github/workflows/test.yml`
- `.github/workflows/release.yml`
- `tests/wit_compat.rs`

Persistence and fallback defaults matter because they can report stale WIT
versions even when the curated extension matrix is already correct. The main
files to inspect are:

- `migrations/V10__wasm_versioning.sql`
- `src/db/libsql_migrations.rs`
- `src/tools/wasm/storage.rs`
- `src/channels/wasm/storage.rs`
- `src/extensions/manager.rs`

## Constraints

- Do not change the host WIT package version away from `0.3.0` unless a new,
  explicit user request says otherwise.
- Treat this as an alignment plan for the current host contract, not a
  speculative migration to a new WIT package version or a new `wit-bindgen`
  major.
- Keep compatibility enforcement honest. Do not weaken
  `check_wit_version_compat` merely to silence stale installed artifacts.
- Only bump extension artifact versions if files inside the extension source or
  shipped metadata actually change. If the audit proves an extension is already
  correct, avoid unnecessary churn in its `version`.
- Preserve the difference between genuinely historical installed extensions and
  defaults used for newly created records. Historical compatibility handling
  may still need to understand old values, but new default records should not
  imply the host is still on `0.1.0`.
- Keep release packaging truthful. If any extension artifacts or registry
  metadata change, rebuild bundles and refresh checksums from the rebuilt
  archives.
- Use the repository’s existing validation style: prefer the top-level
  `Makefile` targets where they exist, and fall back to direct scripts and
  cargo commands logged through `tee` when no target covers the check.

## Tolerances

- If the audit shows that all curated in-tree extension sources, manifests, and
  release inputs are already aligned to `0.3.0`, do not invent extra work.
  Narrow the implementation to stale runtime defaults, tests, and docs.
- If any curated extension or channel still advertises a pre-`0.3.0` WIT
  version, expand the scope to that specific extension set and bump its regular
  artifact version together with the WIT metadata fix.
- If the user intended sibling repositories such as external plugins outside
  this repo, stop after completing the IronClaw-host plan and ask for the exact
  repos to include. This plan only has direct authority over the current
  repository.
- If release checksums cannot be regenerated from local artifacts after any
  extension metadata change, leave the work partial and call out the exact
  missing bundle paths.

## Risks

- The phrase “all extensions and channels” can mean two different scopes:
  curated in-tree extensions in this repo, or every external plugin that might
  target IronClaw. Only the first scope is directly visible here.
- The repo currently mixes `wit-bindgen` versions across extensions. That is
  not automatically wrong, but the full matrix build must prove it does not
  hide inconsistent generated bindings under the current WIT contract.
- Some persistence and test surfaces still embed `0.1.0` fallback values. Those
  stale values can mislead operators into thinking the host still defaults to an
  older WIT version.
- The compatibility checker treats `0.x` minors as breaking. That is correct
  for `0.3.0`, but the audit must confirm the tests still describe the current
  rule clearly.
- Registry manifests include release artifact checksums. If any shipped
  extension metadata changes and the checksums are not refreshed, installs from
  the curated registry will drift.

## Milestone 1: Audit the current `0.3.0` matrix and capture evidence

Begin by proving the current state rather than editing files immediately.

1. Verify that `wit/tool.wit`, `wit/channel.wit`,
   `src/tools/wasm/mod.rs`, every curated extension sidecar, and every curated
   registry manifest already advertise `0.3.0`.
2. Rebuild the full extension matrix with
   `./scripts/build-wasm-extensions.sh`.
3. Run `cargo test --all-features --test wit_compat -- --nocapture` to prove the built
   matrix instantiates against the current host linker.
4. Record which surfaces are already aligned and which ones still leak older
   values such as `0.1.0`.

This milestone succeeds when the repo has a concrete alignment inventory backed
by command evidence. If the audit finds no drift in the curated matrix, later
milestones should focus on stale defaults, reporting, and docs rather than
touching every extension directory.

Suggested commands:

```bash
BRANCH=$(git branch --show-current | tr '/' '-')
set -o pipefail && \
  rg -n 'near:agent@0\.3\.0|"wit_version"\s*:\s*"0\.3\.0"' \
  wit src/tools/wasm/mod.rs tools-src channels-src registry 2>&1 | \
  tee /tmp/audit-wit-versions-ironclaw-${BRANCH}.out
set -o pipefail && \
  ./scripts/build-wasm-extensions.sh 2>&1 | \
  tee /tmp/build-wasm-extensions-ironclaw-${BRANCH}.out
set -o pipefail && \
  cargo test --all-features --test wit_compat -- --nocapture 2>&1 | \
  tee /tmp/test-wit-compat-ironclaw-${BRANCH}.out
```

Commit boundary after this milestone: only if files needed to change because
the audit found actual drift.

## Milestone 2: Fix stale runtime defaults and reporting paths

Once the curated extension matrix is proven, inspect the host paths that still
surface stale WIT defaults.

Files to inspect first:

- `migrations/V10__wasm_versioning.sql`
- `src/db/libsql_migrations.rs`
- `src/extensions/manager.rs`
- `src/tools/wasm/loader.rs`
- `src/tools/builtin/extension_tools.rs`

The implementation goal is narrow:

1. Replace stale `0.1.0` defaults when they describe newly created rows or
   newly reported host expectations.
2. Keep compatibility handling for genuinely older installed artifacts intact.
3. Update tests so host-facing info and upgrade flows consistently describe the
   current contract as `0.3.0`.

Be careful here. A stored historical artifact may still legitimately contain
`0.1.0`, but a fresh record created under the current host should not default
to that value unless there is a documented backward-compatibility reason.

Suggested commands:

```bash
BRANCH=$(git branch --show-current | tr '/' '-')
set -o pipefail && \
  rg -n '0\.1\.0|0\.3\.0' \
  migrations src/db src/extensions src/tools/wasm src/channels/wasm 2>&1 | \
  tee /tmp/audit-stale-wit-defaults-ironclaw-${BRANCH}.out
set -o pipefail && \
  cargo test --all-features test_upgrade_up_to_date_extension -- --nocapture \
  2>&1 | tee /tmp/test-upgrade-up-to-date-ironclaw-${BRANCH}.out
set -o pipefail && \
  cargo test --all-features test_upgrade_outdated_not_in_registry -- \
  --nocapture 2>&1 | \
  tee /tmp/test-upgrade-outdated-ironclaw-${BRANCH}.out
```

Commit boundary after this milestone: one commit for default, reporting, and
test fixes.

## Milestone 3: Refresh operator and contributor guidance

After runtime behavior is truthful, make the human-facing guidance match the
actual current contract.

Files to review:

- `docs/BUILDING_CHANNELS.md`
- `src/tools/README.md`
- `README.md`
- `README.zh-CN.md`
- `tests/wit_compat.rs`

The key outcome is that contributors can tell, from repository docs and test
guidance alone, that new or rebuilt IronClaw extensions and channels must
target WIT `0.3.0`.

If the audit in Milestone 1 found zero extension-matrix drift, this milestone
may be mostly wording and evidence updates. If Milestone 2 changed no
extension-side artifacts, do not republish or checksum-bump bundles just to
create motion.

Suggested commands:

```bash
BRANCH=$(git branch --show-current | tr '/' '-')
set -o pipefail && \
  bunx markdownlint-cli2 \
  docs/BUILDING_CHANNELS.md \
  docs/plans/2026-03-09-use-wit-v3-in-extensions.md 2>&1 | \
  tee /tmp/markdownlint-ironclaw-${BRANCH}.out
set -o pipefail && \
  cargo test --all-features wit_compat_all_registry_extensions_have_source \
  -- --nocapture 2>&1 | \
  tee /tmp/test-registry-sources-ironclaw-${BRANCH}.out
```

Commit boundary after this milestone: one docs-and-tests commit if files
changed.

## Milestone 4: Rebuild and republish only if extension artifacts changed

This milestone is conditional. Only execute it if Milestone 1 or Milestone 2
required edits inside extension source directories, sidecar capabilities files,
or registry manifests that ship with release bundles.

If that happens:

1. Rebuild the affected extension archives.
2. Refresh all affected registry `sha256` values from the rebuilt archives.
3. Verify `.github/workflows/release.yml` still packages the same bundle names.

If no shipped extension artifacts changed, explicitly record that this
milestone was skipped because the curated matrix was already at WIT `0.3.0`.

Suggested commands:

```bash
BRANCH=$(git branch --show-current | tr '/' '-')
set -o pipefail && \
  ./scripts/build-wasm-extensions.sh 2>&1 | \
  tee /tmp/build-release-bundles-ironclaw-${BRANCH}.out
```

Commit boundary after this milestone: one release-artifact sync commit if and
only if shipped bundles or registry manifests changed.

## Validation and acceptance evidence

Before declaring the alignment complete, capture evidence for all of the
following:

1. `wit/tool.wit`, `wit/channel.wit`, and `src/tools/wasm/mod.rs` still report
   `0.3.0`.
2. Every curated in-tree sidecar and registry manifest still reports
   `wit_version: "0.3.0"`.
3. `./scripts/build-wasm-extensions.sh` succeeds for the curated matrix.
4. `cargo test --all-features --test wit_compat -- --nocapture` succeeds.
5. Any stale host-default or info-path `0.1.0` values that were misleading for
   new records have been removed or documented.
6. If any shipped extension artifacts changed, the registry checksums come from
   rebuilt archives.

Record the exact log paths in the final implementation summary.

## Progress

- [x] 2026-03-09T16:11:14+00:00 Drafted the initial execplan after scoping the
  current WIT surfaces, extension matrix, runtime compatibility checks, CI
  jobs, and release packaging.
- [x] 2026-03-09T16:50:14+00:00 Corrected the plan target from `3.0.0` to
  `0.3.0` after re-checking the repository state. The current host contract and
  curated in-tree extension matrix are already on `0.3.0`.
- [x] 2026-03-09T17:08:00+00:00 Audited the live host contract and curated
  matrix. `wit/tool.wit`, `wit/channel.wit`, `src/tools/wasm/mod.rs`, curated
  sidecar capabilities files, and curated registry manifests already report
  WIT `0.3.0`.
- [x] 2026-03-09T17:15:00+00:00 Rebuilt the curated extension matrix with
  `./scripts/build-wasm-extensions.sh`; the build completed successfully and
  wrote evidence to
  `/tmp/build-wasm-extensions-ironclaw-use-wit-v3-in-extensions.out`.
- [x] 2026-03-09T17:18:00+00:00 Re-ran
  `cargo test --all-features --test wit_compat -- --nocapture` after the full matrix
  build. All three compatibility tests passed, including the WhatsApp channel,
  with evidence in
  `/tmp/test-wit-compat-rerun-ironclaw-use-wit-v3-in-extensions.out`.
- [x] 2026-03-09T17:39:00+00:00 Fixed stale runtime defaults and reporting
  paths that implied older WIT versions. The implementation updated the
  libSQL base schema, added a libSQL incremental migration for upgraded
  databases, and added PostgreSQL migration
  `migrations/V12__wasm_wit_default_0_3_0.sql`, all covered by red-green unit
  plus behavioural tests.
- [x] 2026-03-09T17:29:00+00:00 Added a behavioural integration test,
  `tests/libsql_wit_defaults_integration.rs`, that seeds a legacy libSQL
  schema, runs `run_migrations()`, and asserts that inserts without an explicit
  `wit_version` now land as `0.3.0`. The red run failed as expected before the
  migration fix, with evidence in
  `/tmp/test-behavior-wit-defaults-red-ironclaw-use-wit-v3-in-extensions.out`.
- [x] 2026-03-09T17:39:00+00:00 Patched the libSQL base schema and incremental
  migrations so stale `0.1.0` defaults are replaced with `0.3.0`, and added
  PostgreSQL migration `migrations/V12__wasm_wit_default_0_3_0.sql` for the
  same default shift on the PostgreSQL path.
- [x] 2026-03-09T17:39:00+00:00 Re-ran the behavioural upgrade test after the
  migration fix. It passed and wrote evidence to
  `/tmp/test-behavior-wit-defaults-green-ironclaw-use-wit-v3-in-extensions.out`.
- [x] 2026-03-09T17:33:00+00:00 Imported the sibling authoring guide as
  `docs/writing-web-assembly-tools-for-ironclaw.md` and rewrote it around the
  actual IronClaw `0.3.0` WIT format, sidecar `wit_version`, packaging
  contract, and host capability limitations.
- [x] 2026-03-09T17:47:00+00:00 Added and passed focused unit tests for the
  libSQL schema and V10 incremental migration content in
  `src/db/libsql_migrations.rs`, with green evidence in
  `/tmp/test-unit-wit-defaults-green-lib-ironclaw-use-wit-v3-in-extensions.out`
  and
  `/tmp/test-unit-wit-defaults-green-lib-v10-ironclaw-use-wit-v3-in-extensions.out`.
- [x] 2026-03-09T17:54:00+00:00 Re-ran markdown lint on the living plan and
  imported guide. It passed with evidence in
  `/tmp/markdownlint-ironclaw-use-wit-v3-in-extensions.out`.
- [x] 2026-03-09T17:54:00+00:00 Re-ran the repo-level WIT compatibility gate
  in a focused target directory with
  `cargo test --all-features --test wit_compat -- --nocapture`. All five tests
  passed with evidence in
  `/tmp/test-wit-compat-focused-ironclaw-use-wit-v3-in-extensions.out`.
- [x] 2026-03-09T17:55:20+00:00 Committed the implementation as `6bf4933`.
- [x] 2026-03-09T17:55:20+00:00 Skipped release-bundle regeneration because no
  shipped extension source, sidecar capabilities file, or registry manifest
  changed. The work only touched host migrations, tests, the living plan, and
  the imported authoring guide.

## Surprises & Discoveries

- The branch and plan filename say “v3”, but the actual repository target is
  WIT `0.3.0`. The document must therefore be treated as an alignment plan, not
  a major-version migration plan.
- The curated in-tree extension matrix already advertises `0.3.0` in its
  sidecar capabilities files and registry manifests.
- The repo has a top-level `Makefile`, but some validation is still script-first
  and cargo-first when no matching make target exists.
- All in-tree tools and channels import the shared WIT files directly with
  `wit_bindgen::generate!`, which makes the matrix audit meaningful across the
  full curated set.
- The stale `0.1.0` drift is not in the curated extension matrix. It is in
  persistence defaults and fallback surfaces, specifically
  `migrations/V10__wasm_versioning.sql` and `src/db/libsql_migrations.rs`.
- The installed-extension upgrade tests in `src/extensions/manager.rs` still
  use `0.1.0` for deliberately historical fixtures. Those fixtures should stay
  historical; the work is to stop fresh records and docs from implying that
  `0.1.0` is still current.
- The user added a non-negotiable test requirement during implementation:
  any IronClaw application change must be protected by both unit tests and
  behavioural tests, and the preferred workflow is red-green rather than
  edit-first patching.
- The user also expanded the docs scope: import
  `../imap-wasm/docs/writing-web-assembly-tools-for-ironclaw.md` into `docs/`
  and update it with the findings from this alignment pass, especially the
  WIT `0.3.0` format and packaging realities.
- Several older repository docs already fail the current markdownlint profile.
  To keep this WIT change set gateable, the doc edits were narrowed back to the
  imported guide and the living plan instead of expanding into an unrelated
  documentation cleanup.

## Decision Log

- 2026-03-09T16:11:14+00:00: Initial draft assumed “WIT 3.0” meant a package
  version jump from `near:agent@0.3.0` to `near:agent@3.0.0`.
- 2026-03-09T16:50:14+00:00: Replace that assumption. The correct target is
  WIT `0.3.0`, so the plan now focuses on proving and completing alignment to
  the current host contract rather than changing the host contract itself.
- 2026-03-09T16:50:14+00:00: Keep the extension-matrix scope repo-local unless
  the user later names external plugin repositories explicitly.
- 2026-03-09T16:50:14+00:00: Only rebuild and republish bundles if extension
  artifacts or registry manifests actually change during the alignment work.
- 2026-03-09T17:08:00+00:00: Treat the current implementation scope as three
  coordinated deliverables: truthful WIT defaults and migration behaviour,
  red-green unit plus behavioural coverage for those application changes, and
  an imported authoring guide under `docs/` that documents the actual WIT
  `0.3.0` format.
- 2026-03-09T17:33:00+00:00: Keep the docs scope focused on the imported guide
  and the plan after `markdownlint-cli2` showed broad pre-existing violations in
  other repository docs. Avoid turning this WIT alignment pass into a general
  markdown remediation branch.

## Outcomes & Retrospective

Implementation is complete.

- Already aligned at `0.3.0`: the host WIT package declarations, host version
  constants, curated extension sidecars, curated registry manifests, and the
  built extension matrix.
- Fixed in this change set: stale `0.1.0` defaults for new and upgraded
  `wasm_tools.wit_version` and `wasm_channels.wit_version` records via
  PostgreSQL migration `V12` plus libSQL base-schema and incremental-migration
  updates.
- Test evidence:
  `/tmp/test-behavior-wit-defaults-red-ironclaw-use-wit-v3-in-extensions.out`
  captured the expected red failure before the migration fix.
  `/tmp/test-behavior-wit-defaults-green-ironclaw-use-wit-v3-in-extensions.out`
  captured the behavioural upgrade-path pass after the fix.
  `/tmp/test-unit-wit-defaults-green-lib-ironclaw-use-wit-v3-in-extensions.out`
  and
  `/tmp/test-unit-wit-defaults-green-lib-v10-ironclaw-use-wit-v3-in-extensions.out`
  captured the focused unit-test passes.
  `/tmp/test-wit-compat-focused-ironclaw-use-wit-v3-in-extensions.out`
  captured the repo-level WIT compatibility gate passing.
  `/tmp/markdownlint-ironclaw-use-wit-v3-in-extensions.out`
  captured markdown lint passing for the updated plan and imported guide.
- Release bundles were correctly left unchanged because no shipped extension
  artifact inputs changed.
