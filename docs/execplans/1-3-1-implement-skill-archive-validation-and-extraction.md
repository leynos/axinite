# Implement `.skill` archive validation and extraction

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

Roadmap item `1.3.1` exists to replace the current best-effort ZIP
shortcut with a real skill-bundle contract. Today axinite can download
an archive and pull out a single root `SKILL.md`, but it discards
everything else and does not enforce the narrow bundle shape described in
RFC 0003. That is too weak for later multi-file skill work, and it risks
accepting archive contents that the runtime should never install.

After this work, the installer must accept only passive `.skill` bundles
whose entries all share one top-level `skill-name` prefix with `SKILL.md`
at `<root>/SKILL.md`, plus optional `references/` and `assets/` content.
It must reject unsupported top-level content, nested `SKILL.md` files,
executable payloads, path traversal, special-file entries, duplicate
normalized paths, and size or count overflows. Extraction must be staged
and atomic so a failed install never leaves a partial skill tree behind.

For the avoidance of doubt, a valid bundle tree should look like this
before it is zipped into `deploy-docs.skill`:

```plaintext
deploy-docs/
├── SKILL.md
├── references/
│   ├── usage.md
│   └── troubleshooting/
│       └── api-errors.md
└── assets/
    ├── logo.png
    └── prompt-template.txt
```

Success is observable in five ways. First, bundle validation returns
typed, user-facing errors that explain why an invalid archive was
rejected. Second, a valid archive installs to
`<install-root>/<skill-name>/SKILL.md` with its allowed ancillary files
preserved. Third, the existing install surfaces continue to share one
implementation path instead of growing separate archive rules in the chat
tool, the web handler, and the fetch helper. Fourth, unit tests with
`rstest` cover happy paths, unhappy paths, and edge cases for validation,
staging, and commit behaviour. Fifth, behavioural coverage with
`rstest-bdd` is added only if one focused in-process scenario is
proportional; otherwise the non-applicability is documented explicitly.

This plan uses `hexagonal-architecture` narrowly. Bundle-shape policy,
allowed-path rules, and typed validation results belong in the skills
subsystem. HTTP download, ZIP decoding, and filesystem staging are
adapters around that policy. The goal is a clean contract boundary, not a
pattern transplant across the whole application.

## Approval gates

- Plan approved
  Acceptance criteria: the implementation remains scoped to roadmap item
  `1.3.1`, with install-surface widening (`.skill` uploads, `.skill`
  URLs, canonical bundle roots in the loaded-skill model, and
  `skill_read_file`) left to roadmap items `1.3.2` through `1.3.4`.
  Sign-off: human reviewer approves this ExecPlan before implementation
  begins.

- Implementation complete
  Acceptance criteria: archive validation, typed errors, staged
  extraction, shared installer integration, and the agreed test coverage
  are all in place without expanding the runtime surface beyond this
  slice.
  Sign-off: implementer marks all milestones complete before final
  validation.

- Validation passed
  Acceptance criteria: targeted tests, `make all`, Markdown linting for
  changed docs, and `git diff --check` all pass with retained logs.
  Sign-off: implementer records evidence immediately before commit.

- Docs synced
  Acceptance criteria: `docs/roadmap.md`, `docs/users-guide.md`,
  `docs/agent-skills-support.md`, and the relevant architecture notes all
  describe the same bundle-validation rules.
  Sign-off: implementer completes the documentation sync before marking
  the roadmap item done.

## Repository orientation

The files below are the key orientation points for this roadmap item.

- `docs/roadmap.md` defines `1.3.1`, its dependency position before
  `2.2`, and the success rule that only bundles with one shared
  top-level prefix and `SKILL.md` at `<root>/SKILL.md` are accepted.
- `docs/rfcs/0003-skill-bundle-installation.md` is the design authority.
  The most important sections are `Proposed Bundle Format`,
  `Validation And Extraction`, and `Rollout Plan`.
- `docs/agent-skills-support.md` is the maintainer-facing architecture
  document for the current skills subsystem. It currently documents the
  single-file model and explicitly states that ZIP installs collapse to
  one `SKILL.md`.
- `docs/users-guide.md` is the operator-facing document that must explain
  the installed bundle rules, limits, and failure behaviour once this
  slice lands.
- `docs/axinite-architecture-overview.md` is the relevant high-level
  architecture reference. The request named
  `docs/axinite-architecture-summary.md`, but that file does not exist in
  this checkout.
- `src/skills/registry.rs` owns discovery, install preparation,
  in-memory commit, and remove flows today. Functions such as
  `prepare_install_to_disk`, `commit_install`, and `install_skill`
  currently assume a single `SKILL.md` payload.
- `src/skills/parser.rs` owns `SKILL.md` parsing and remains the
  authority for the prompt-body contract after a bundle's root entrypoint
  has been validated and staged.
- `src/tools/builtin/skill_fetch/http.rs` owns the Server-Side Request
  Forgery (SSRF)-safe download
  path for remote skill installs.
- `src/tools/builtin/skill_fetch/zip_extract.rs` owns the current
  archive shortcut. It currently returns only the first root `SKILL.md`
  content and discards all sibling files.
- `src/tools/builtin/skill_tools/install.rs` and
  `src/channels/web/handlers/skills.rs` are the two existing install
  adapters. They must continue to share one archive-validation and
  staging path.
- `src/tools/builtin/skill_fetch/tests.rs`,
  `src/skills/registry.rs` tests, and
  `src/tools/builtin/skill_tools/tests.rs` are the nearest existing Rust
  test seams for archive policy, install behaviour, and tool contracts.
- `docs/rust-testing-with-rstest-fixtures.md`,
  `docs/reliable-testing-in-rust-via-dependency-injection.md`,
  `docs/rstest-bdd-users-guide.md`,
  `docs/rust-doctest-dry-guide.md`, and
  `docs/complexity-antipatterns-and-refactoring-strategies.md` are the
  requested implementation references for tests, dependency injection,
  BDD proportionality, public API documentation, and keeping the new
  validator small and comprehensible.
- `FEATURE_PARITY.md` includes the skills parity row and must be checked
  for drift when this roadmap item changes shipped behaviour.

## Constraints

- Keep `1.3.1` scoped to archive validation and extraction. Do not add
  new upload endpoints, new `.skill` URL affordances, loaded-skill root
  metadata, or `skill_read_file` in this slice.

- Keep bundle policy inward. Allowed paths, executable rejection, root
  prefix rules, duplicate-path detection, and typed error definitions
  must live in the skills subsystem rather than being reinvented in HTTP
  or web-handler code.

- Preserve the current single logical install flow. The chat tool and the
  web handler may remain separate adapters, but they must call the same
  shared archive-preparation code path.

- Do not add a new external dependency. Use the existing workspace
  dependencies already available in `Cargo.toml`, including the current
  `zip` crate if a central-directory parser is needed.

- Keep installs staged and atomic. A failed archive validation or failed
  extraction must not leave a partially written skill tree inside the
  install root.

- Preserve compatibility with dotted skill names at archive validation
  time, as RFC 0003 requires.

- Keep the prompt contract unchanged in this slice. Runtime activation
  still injects only the selected `SKILL.md` body until roadmap items
  `1.3.3` and `1.3.4` land.

- Use `rstest` fixtures for shared unit and integration setup.

- Use `rstest-bdd` only if one focused in-process scenario can be added
  without introducing a new behaviour harness family for skills. If that
  is not proportional, document the decision and keep the behavioural
  proof in `rstest`.

- Avoid ambient environment mutation and live network dependencies in
  tests. Prefer temp directories, in-memory fixtures, and injected
  adapters.

- Update operator-facing and maintainer-facing documentation in the same
  delivery pass, and mark roadmap item `1.3.1` done only after the code,
  tests, and docs agree.

## Tolerances (exception triggers)

- Scope: if the smallest credible implementation touches more than
  15 files or roughly 800 net new lines before documentation, stop and
  confirm that `1.3.2` or `1.3.3` work has not leaked into this slice.

- Interface: if supporting staged bundle extraction requires changing the
  public request schema for `skill_install`, the web install endpoint, or
  the loaded-skill runtime model, stop and decide whether the work has
  crossed into a later roadmap item.

- Dependency: if the current `zip` crate or existing utilities cannot
  express the required validation safely and a new archive dependency
  appears necessary, stop and review that explicitly rather than adding
  it as an implementation detail.

- Atomicity: if the implementation cannot guarantee cleanup after a
  failed extraction with one focused staging refactor, stop and document
  the hidden filesystem coupling before continuing.

- Behaviour-driven development (BDD) proportionality: if adding
  `rstest-bdd` would require more than one feature file, more than one
  new scenario-support module, or a new end-to-end harness, record the
  non-applicability and keep behavioural proof in `rstest`.

- Ambiguity: if the existing code leaves it unclear whether a case belongs
  in `1.3.1` or a later bundle task, stop and record the overlap rather
  than silently broadening scope.

- Documentation drift: if RFC 0003, `docs/agent-skills-support.md`, and
  `docs/users-guide.md` cannot be reconciled without a larger design
  change, stop and resolve the wording before the roadmap entry is marked
  done.

## Risks

- Risk: The current archive path lives under the tool-fetch adapter, so
  the implementation could accidentally leave the validation rules tied to
  remote downloads instead of making them reusable for every install
  surface.
  Severity: high
  Likelihood: medium
  Mitigation: introduce a shared bundle-validation service in
  `src/skills/` and keep the fetch helper limited to obtaining bytes.

- Risk: The current install path writes `SKILL.md` directly into the final
  directory, which makes atomic multi-file extraction easy to get wrong.
  Severity: high
  Likelihood: medium
  Mitigation: add an explicit staging directory and commit step before
  wiring the adapters, then test cleanup on failure.

- Risk: The existing manual local-header parser may not expose enough
  metadata to detect every unsupported archive case cleanly, especially
  duplicate normalized paths and special-file entries.
  Severity: medium
  Likelihood: medium
  Mitigation: prefer the existing `zip` dependency for archive inspection
  if it keeps the validator smaller and safer than extending the ad hoc
  parser.

- Risk: Behaviour-driven coverage may add more scaffolding than value
  because this subsystem currently has no skill-focused `rstest-bdd`
  harness.
  Severity: medium
  Likelihood: high
  Mitigation: evaluate BDD proportionality explicitly, and keep the
  authoritative regression proof in `rstest` if a narrow scenario is not
  practical.

- Risk: Documentation drift is already present. The maintainer-facing
  skills document still describes a single-file system and the requested
  architecture-summary file is absent.
  Severity: medium
  Likelihood: high
  Mitigation: treat documentation synchronization as its own milestone and
  record the missing-file discovery in the decision log.

## Milestone 1: confirm the precise `1.3.1` contract and the shared seam

Start by restating the implementation boundary in code terms before making
changes.

1. Re-read RFC 0003 sections `Proposed Bundle Format`,
   `Validation And Extraction`, and `Rollout Plan` together with roadmap
   items `1.3.1` through `1.3.5`.
2. Confirm which existing install surfaces must consume the new shared
   validator now: the chat tool, the web handler, and any current URL ZIP
   path that already reaches `fetch_skill_content`.
3. Record the cases that are intentionally out of scope for this slice:
   uploaded bundles, explicit `.skill` URLs, runtime bundle-root metadata,
   and lazy file reads.
4. Choose the common install-preparation seam. The preferred shape is a
   shared service under `src/skills/` that accepts either raw `SKILL.md`
   content or archive bytes, validates them, stages the install tree, and
   returns the prepared `LoadedSkill` plus the finalized install path.

Expected result: the implementer can name one shared preparation contract
that later roadmap items can extend without redoing the install pipeline.

## Milestone 2: introduce a typed bundle-validation model in `src/skills/`

Create a dedicated bundle module in the skills subsystem that owns the
bundle rules rather than scattering them across adapters.

1. Add a new module such as `src/skills/bundle.rs` or a similarly named
   pair of files. Keep the public surface small.
2. Define typed inputs and outputs for validation, for example a raw
   archive view, a `ValidatedSkillBundle` result, and a typed
   `SkillBundleError` enum that can be wrapped into `SkillRegistryError`
   and tool/web-facing errors.
3. Encode the RFC rules in one place:
   one top-level prefix with `SKILL.md` at `<root>/SKILL.md`, only
   `references/` and `assets/`, no nested `SKILL.md`, no `scripts/` or
   `bin/`, no executables, no absolute or traversal paths, no special
   file types, no duplicate normalized paths, bounded file sizes,
   bounded total size, bounded file count, and UTF-8 for text that must
   be parsed as text.
4. Keep root-name validation aligned with RFC 0003, including dotted
   names and the 64-byte maximum before normalization.
5. Ensure the validator returns enough structured information for later
   extraction without teaching the adapters how to classify archive
   paths.

Expected result: there is one authoritative policy module for archive
shape and one error vocabulary that all install adapters can share.

## Milestone 3: add staged extraction and atomic commit

Once the bundle is validated, extraction must become a filesystem
adapter concern rather than part of archive policy.

1. Refactor `SkillRegistry::prepare_install_to_disk` into a higher-level
   staged install flow that can write either a single-file skill or a
   validated bundle tree into a temporary directory under the install
   root.
2. Add an explicit commit step that renames or otherwise atomically moves
   the staged tree into `<install-root>/<skill-name>`.
3. Preserve the current split-lock pattern: prepare asynchronously
   without holding the registry lock, then commit the in-memory
   registration under a brief write lock.
4. Reuse `load_and_validate_skill` against the staged `SKILL.md` so the
   installed prompt still round-trips through the existing parser and
   gating logic.
5. Ensure failure cleanup is tested, especially when a bundle passes
   archive validation but later fails during filesystem staging.

Expected result: multi-file extraction is atomic, round-tripped through
the existing skill parser, and safe to reuse from every install surface.

## Milestone 4: rewire install adapters to use the shared bundle path

Move the current archive shortcut out of the tool-specific fetch path and
into the shared install logic.

1. Narrow `src/tools/builtin/skill_fetch/http.rs` so it becomes a safe
   byte-fetching adapter. It may still sniff archive bytes, but it must
   stop owning bundle-policy decisions.
2. Replace or retire the current `extract_skill_from_zip` helper once the
   shared skills-side bundle validator can inspect and extract archive
   contents correctly.
3. Update `SkillInstallTool::execute` and
   `skills_install_handler` so they both call the same preparation path
   instead of parsing `SKILL.md` up front and assuming a single file.
4. Keep the outward approval and routing behaviour stable in this slice.
   The change is the install contract, not the transport shape.
5. Preserve or improve current error fidelity so invalid bundles surface
   clear messages rather than generic fetch or parse failures.

Expected result: the tool path and the web path stay aligned, and bundle
rules are no longer trapped inside a remote-download helper.

## Milestone 5: add regression coverage with `rstest`

Lock the new policy down with tests before marking the roadmap entry
done.

1. Add focused unit tests for the new bundle validator using `rstest`
   cases. Cover at least:
   valid bundle with `references/` and `assets/`,
   missing `<root>/SKILL.md` under the shared prefix,
   multiple top-level prefixes,
   unexpected top-level content,
   nested `SKILL.md`,
   `scripts/` or `bin/`,
   executable extensions,
   path traversal and absolute paths,
   duplicate normalized paths or case-fold collisions,
   oversize entry, oversize archive, excessive file count,
   and dotted skill names that must remain valid.
2. Extend registry install tests so a validated bundle is staged,
   committed, and discoverable through the existing `SKILL.md` load path,
   while invalid bundles leave no partial directory behind.
3. Extend tool and web-handler tests only where they catch adapter-level
   regressions not already covered by the validator and registry tests.
4. Keep helpers local to the subsystem. If a single test file approaches
   the repository size limit, extract fixture builders into a nearby
   support module.

Expected result: bundle policy and install atomicity are both enforced by
deterministic Rust tests that fail before later tasks build on the new
bundle format.

## Milestone 6: evaluate one focused `rstest-bdd` scenario for installer behaviour

The user asked for behavioural coverage where behaviour-driven
development is applicable. Evaluate that explicitly instead of assuming
it.

1. Check whether one in-process skill-install scenario can reuse the new
   staged install service and temp-directory fixtures without introducing
   a broader harness family.
2. If it can, add one feature file and one Rust scenario module for a
   behaviour such as:

   ```plaintext
   Feature: skill bundle installation

     Scenario: valid bundle installs and invalid executable bundle is rejected
       Given a valid passive skill bundle
       When the installer prepares and commits it
       Then the installed skill tree contains SKILL.md and bundled references
       When the installer receives a bundle with executable content
       Then the install fails with an invalid_skill_bundle error
   ```

3. If that scenario would require disproportionate scaffolding, record in
   `Decision Log` that `rstest-bdd` is not applicable for this slice and
   keep the behavioural proof in the `rstest` integration tests instead.

Expected result: the plan either lands one proportional BDD scenario or
documents clearly why `rstest` is the right proof mechanism here.

## Milestone 7: synchronize design, user, and roadmap documents

The code is not done until the documents stop contradicting each other.

1. Update `docs/agent-skills-support.md` so it no longer describes ZIP
   installs as single-file extraction only, and explain the new shared
   bundle-validation and staging boundary.
2. Update `docs/users-guide.md` with the operator-visible bundle rules:
   accepted layout, disallowed content, and install failure behaviour.
3. Update the relevant architecture note. Use
   `docs/axinite-architecture-overview.md` if the high-level runtime story
   needs a brief note about validated multi-file skill bundles. Update RFC
   0003 only if implementation decisions materially clarify or constrain
   the proposed design.
4. Check `FEATURE_PARITY.md` and update it if the skills row or notes now
   describe stale shipped behaviour.
5. Mark roadmap item `1.3.1` done in `docs/roadmap.md` only after the
   implementation, tests, and docs are all complete.
6. Update `docs/contents.md` if new documentation files or plan entries
   were added during the work.

Expected result: the roadmap, design docs, and user guide all describe
the same bundle-validation contract.

## Milestone 8: validate, record evidence, and commit

Run the validation sequence sequentially, retain logs with `tee`, and
record the decisive evidence in this plan before committing.

1. Run targeted Rust tests for the bundle validator, registry install
   flow, and adapter-level regressions. Use stable `/tmp` log names keyed
   by the branch name.

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   cargo test skill_fetch \
     | tee /tmp/test-skill-fetch-axinite-${BRANCH_SLUG}.out
   cargo test skill_install \
     | tee /tmp/test-skill-install-axinite-${BRANCH_SLUG}.out
   cargo test skills::registry \
     | tee /tmp/test-skill-registry-axinite-${BRANCH_SLUG}.out
   ```

2. If a proportional `rstest-bdd` scenario was added, run its filtered
   test target and retain the log in `/tmp`.
3. Run the repository gate.

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   make all | tee /tmp/make-all-axinite-${BRANCH_SLUG}.out
   ```

4. Run Markdown validation for every changed document rather than a
   hardcoded path list.

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   CHANGED_DOCS=$(git diff --name-only HEAD -- '*.md')
   if [ -n "${CHANGED_DOCS}" ]; then
     printf '%s\n' "${CHANGED_DOCS}" \
       | xargs bunx markdownlint-cli2 \
       | tee /tmp/markdownlint-axinite-${BRANCH_SLUG}.out
   fi
   ```

5. Run the diff sanity check.

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   git diff --check | tee /tmp/git-diff-check-axinite-${BRANCH_SLUG}.out
   ```

Expected result: the implementation has retained, reviewable evidence for
tests, full gates, documentation linting, and clean diffs.

## Progress

- [x] 2026-04-17T20:45:00+02:00: Drafted the ExecPlan for roadmap item
  `1.3.1` and captured the current installer, archive, test, and
  documentation seams.
- [x] 2026-04-18T11:40:00+02:00: Added `src/skills/bundle/` as the
  shared `.skill` policy boundary with typed bundle-validation errors,
  bounded archive rules, and `rstest` unit coverage for happy and unhappy
  paths.
- [x] 2026-04-18T12:05:00+02:00: Refactored `SkillRegistry` installs into a
  staged transaction that prepares a temporary install tree, round-trips the
  staged `SKILL.md` through the existing parser, and atomically renames the
  staged directory into place during commit.
- [x] 2026-04-18T12:20:00+02:00: Rewired the tool and web install adapters to
  fetch raw bytes, delegate archive policy to the shared registry path, and
  clean up failed staged installs.
- [x] 2026-04-18T12:35:00+02:00: Added focused `rstest` coverage for bundle
  validation, registry bundle installs, staged cleanup, and install-tool
  regressions.
- [x] 2026-04-18T12:50:00+02:00: Synchronized the roadmap, user's guide,
  skills architecture document, and high-level architecture overview with the
  implemented bundle-validation contract.
- [x] 2026-04-18T13:35:00+02:00: Final validation passed. Targeted bundle,
  registry, install-tool, and fetch tests were green; `make all`,
  `bunx markdownlint-cli2` over changed docs, and `git diff --check`
  all passed with retained `/tmp` logs.

## Surprises & Discoveries

- 2026-04-17T20:10:00+02:00: The requested
  `docs/axinite-architecture-summary.md` file is not present in this
  checkout. `docs/axinite-architecture-overview.md` is the relevant
  replacement.
- 2026-04-17T20:16:00+02:00: The current archive helper under
  `src/tools/builtin/skill_fetch/zip_extract.rs` extracts only a single
  root `SKILL.md` and discards sibling files. That matches the current
  maintainer-facing documentation and confirms the need for this roadmap
  item.
- 2026-04-17T20:24:00+02:00: No existing skill-focused `rstest-bdd`
  scenario harness was found in `src/` or `tests/`, so BDD coverage must
  be evaluated for proportionality rather than assumed.
- 2026-04-18T11:55:00+02:00: The cleanest atomic-install seam was to stage a
  full skill tree under the install root and keep the registry write lock only
  around the final same-filesystem rename plus the in-memory insert. That kept
  async filesystem work outside the lock without introducing a broader
  reservation protocol.
- 2026-04-18T12:12:00+02:00: The current runtime model still keys loaded
  skills by the parsed `SKILL.md` manifest name. This slice therefore preserves
  bundled `references/` and `assets/` on disk but intentionally leaves
  canonical bundle-root metadata and lazy file access to roadmap items `1.3.3`
  and `1.3.4`.
- 2026-04-18T13:18:00+02:00: The repository-wide gate remained the correct
  final proof even though this change is tightly scoped. `make all`
  flushed a formatting miss and then validated that the staged install
  seam did not regress broader tooling or channel behaviour.

## Decision Log

- 2026-04-17T20:20:00+02:00: Use `docs/axinite-architecture-overview.md`
  as the high-level architecture reference in this plan because the
  requested summary document does not exist.
  Rationale: the plan must remain executable from the current checkout.

- 2026-04-17T20:32:00+02:00: Keep `1.3.1` strictly scoped to validation
  and extraction, even if the shared preparation seam is designed so
  later tasks can add uploads, `.skill` URLs, bundle-root metadata, and
  `skill_read_file`.
  Rationale: the roadmap intentionally splits those concerns across
  `1.3.2` through `1.3.4`.

- 2026-04-17T20:40:00+02:00: Treat bundle-policy logic as a skills
  subsystem responsibility, not as part of the fetch helper or web/tool
  adapters.
  Rationale: both install adapters already share a registry-driven flow,
  and later bundle inputs will need the same policy without depending on
  HTTP download code.

- 2026-04-18T12:28:00+02:00: Do not add `rstest-bdd` coverage for this slice.
  Rationale: the repository still has no reusable skills-focused BDD harness,
  and the new bundle validator plus staged-registry tests already exercise the
  behaviour in-process without needing a new feature-file or scenario-support
  family.

- 2026-04-18T12:48:00+02:00: Preserve the existing `LoadedSkill` runtime shape
  and record only on-disk bundle preservation in this slice.
  Rationale: adding bundle-root metadata to loaded skills would broaden the
  runtime contract into roadmap item `1.3.3`, while the roadmap only requires
  validated extraction here.

- 2026-04-18T13:32:00+02:00: Leave `FEATURE_PARITY.md` unchanged after review.
  Rationale: the file does not currently track this installer-contract slice at
  a granularity that changed from `❌` to `🚧` or `✅`, so updating it here
  would add noise rather than clarify shipped parity.

## Outcomes & Retrospective

Implemented the shared `.skill` bundle contract for roadmap item `1.3.1`.
axinite now validates passive ZIP-based skill bundles in `src/skills/bundle/`,
stages accepted bundles into a temporary tree, reuses the existing
`SKILL.md` parser and gating path against the staged entrypoint, and commits
the install with an atomic rename. The fetch helper was narrowed to SSRF-safe
byte retrieval, the old single-file ZIP shortcut was retired, and both install
adapters now share the same registry-driven preparation path.

The authoritative regression proof is in `rstest`, not `rstest-bdd`, because
this subsystem still lacks a proportional BDD harness. Validation evidence was
retained in:

- `/tmp/test-skill-bundle-axinite-feat-skill-bundle-execplan.out`
- `/tmp/test-skill-registry-axinite-feat-skill-bundle-execplan.out`
- `/tmp/test-skill-install-axinite-feat-skill-bundle-execplan.out`
- `/tmp/test-skill-fetch-axinite-feat-skill-bundle-execplan.out`
- `/tmp/make-all-axinite-feat-skill-bundle-execplan.out`
- `/tmp/markdownlint-axinite-feat-skill-bundle-execplan.out`
- `/tmp/git-diff-check-axinite-feat-skill-bundle-execplan.out`

Follow-on roadmap items remain necessary for bundle uploads, explicit
`.skill` URL affordances, canonical bundle-root metadata in loaded skills, and
lazy file reads from installed bundles.
