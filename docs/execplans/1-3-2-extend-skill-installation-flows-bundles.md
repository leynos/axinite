# Extend skill installation flows for uploaded bundles and `.skill` URLs

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

Roadmap item `1.3.2` completes the user-facing install paths for passive
multi-file skill bundles. Roadmap item `1.3.1` already added the archive
validator and staged extractor for `.skill` archives, which are ZIP files with
one top-level skill directory, `SKILL.md` at `<root>/SKILL.md`, and optional
`references/` and `assets/` trees. This plan extends the install adapters so
operators can use that validated bundle format from an HTTPS `.skill` URL and
from a local upload in the web Skills page.

After the change, a user can install a bundle such as:

```plaintext
deploy-docs.skill
└── deploy-docs/
    ├── SKILL.md
    ├── references/
    │   └── usage.md
    └── assets/
        └── logo.txt
```

by either entering an HTTPS URL that resolves to the archive or selecting the
local `.skill` file in the browser. In both cases the installed tree under the
configured installed-skills directory preserves `references/` and `assets/`.
When the archive shape is invalid, the user sees the existing explicit
`invalid_skill_bundle: ...` error rather than a generic install failure.

This slice deliberately stops before roadmap items `1.3.3` and `1.3.4`.
Runtime skill activation still injects only the selected `SKILL.md` body until
canonical skill roots and the future `skill_read_file` tool are implemented.

The plan uses `hexagonal-architecture` as boundary discipline, not as a
repository-wide pattern transplant. Bundle policy and canonical install rules
belong in `src/skills/`; HTTP, web forms, JSON requests, and multipart upload
handling are adapters that translate user input into the shared install
service.

## Approval gates

The first gate is plan approval. A human reviewer must explicitly approve this
ExecPlan before implementation starts. Silence is not approval.

The second gate is implementation completion. The implementer must finish the
upload adapter, `.skill` URL path, tests, documentation, and roadmap update
without pulling in runtime file reads or loaded-skill root persistence from
later tasks.

The third gate is validation. The implementer must run the targeted tests while
developing, then the full repository gate `make all` before committing the
feature. All long-running validation commands must be run through `tee` to a
log file under `/tmp`.

The fourth gate is documentation sync. `docs/users-guide.md`,
`docs/agent-skills-support.md`, and the relevant architecture or web-gateway
documents must describe the same install behaviour, and `docs/roadmap.md` must
mark `1.3.2` done only after the feature and validation pass.

## Repository orientation

Start with `docs/contents.md`, `docs/welcome-to-axinite.md`, and
`docs/axinite-architecture-overview.md` for the project direction and runtime
shape. The requested `docs/axinite-architecture-summary.md` is not present in
this checkout; use `docs/axinite-architecture-overview.md` as the available
architecture reference.

`docs/roadmap.md` defines roadmap item `1.3.2`, its dependency on completed
item `1.3.1`, and the success rule for preserving `references/` and `assets/`
while reporting archive-shape failures explicitly.

`docs/rfcs/0003-skill-bundle-installation.md` is the design authority. The
sections to keep open while implementing are `Summary`, `Installation Flows`,
`Canonical skill names and conflict handling`, `Validation And Extraction`,
`UI Changes`, `Testing`, and `Rollout Plan`.

`docs/agent-skills-support.md` is the maintainer-facing component design for
skills. It already states that remote install bytes can be plain `SKILL.md` or
a validated passive `.skill` archive, and it lists the current limitation that
local uploads are not supported.

`docs/users-guide.md` is the operator-facing guide. It currently says this
slice does not yet add local `.skill` uploads; that must change when the
feature lands.

`docs/front-end-architecture.md` and `src/channels/web/CLAUDE.md` describe the
browser gateway. The web Skills tab uses `src/channels/web/static/index.html`,
`src/channels/web/static/app.js`, and `src/channels/web/static/style.css`, and
the API adapter lives in `src/channels/web/handlers/skills.rs`.

`src/skills/bundle/mod.rs`, `src/skills/bundle/path.rs`,
`src/skills/registry/materialize.rs`, and
`src/skills/registry/staged_install.rs` are the completed `1.3.1` core. They
own archive validation, policy errors, materialisation, staging, and atomic
commit preparation. Do not duplicate those rules in web or tool adapters.

`src/tools/builtin/skill_fetch/http.rs` and
`src/tools/builtin/skill_fetch/url_policy.rs` own HTTPS-only and Server-Side
Request Forgery (SSRF) defences for URL installs. `.skill` URL support must
continue to flow through this fetch path.

`src/tools/builtin/skill_tools/install.rs` is the agent-callable
`skill_install` tool. Its current schema still marks `name` as required even
when installing from a URL or inline content. The implementation should make
the install-source contract explicit without forcing a bundle URL to pretend
to have a catalogue name.

`src/channels/web/types.rs` defines `SkillInstallRequest` for JSON installs
and `ActionResponse` for install results. If multipart upload needs a separate
request extractor, keep the user-visible response compatible with existing web
install results.

The requested testing references are:

- `docs/rust-testing-with-rstest-fixtures.md` for `rstest` fixtures and
  parameterized cases.
- `docs/reliable-testing-in-rust-via-dependency-injection.md` for avoiding
  live network and global-state coupling.
- `docs/rstest-bdd-users-guide.md` for proportional behaviour tests with
  `rstest-bdd`.
- `docs/rust-doctest-dry-guide.md` if public APIs are added or documented.
- `docs/complexity-antipatterns-and-refactoring-strategies.md` when deciding
  whether an adapter needs extraction rather than another branch in a long
  handler.

The skills to signpost for implementers are:

- `leta`: use it for symbol navigation and reference checks before editing
  Rust code.
- `rust-router`: use it before Rust implementation work to choose any more
  specific Rust skill.
- `rust-errors`: use it for typed archive and adapter error mapping.
- `rust-types-and-apis`: use it if the install request or payload enum changes.
- `domain-web-services`: use it for the Axum multipart endpoint and web API
  contract.
- `hexagonal-architecture`: use it to keep bundle policy in `src/skills/` and
  transport-specific parsing in adapters.
- `commit-message`: use it when committing the approved implementation.

## Constraints

The implementation must not begin until this plan is approved.

The feature depends on roadmap item `1.3.1`. Before editing, confirm that
`src/skills/bundle/` and the staged install path are present, and avoid
recreating archive validation outside that core.

The install surface must accept exactly one source per request: inline
`content`, direct `url`, catalogue `name` or `slug`, or uploaded `.skill` file.
Ambiguous combinations must fail with a clear validation error.

URL installs must continue to use existing HTTPS-only and SSRF-protected fetch
logic. Do not add a second HTTP client path for `.skill` URLs.

Uploaded `.skill` archives must derive their canonical candidate skill name
only from the archive's shared top-level prefix, as RFC 0003 requires. The
server must not fall back to the uploaded filename, `SKILL.md` metadata, or a
form field when archive shape validation fails.

Both URL and upload installs must use the shared staged install path so that
`references/` and `assets/` are preserved identically.

Archive-shape failures must remain explicit. Do not collapse
`SkillBundleError` messages into a generic "invalid file" or "install failed"
response.

The web endpoint must preserve the `X-Confirm-Action: true` requirement for
mutating installs.

Do not implement `skill_read_file`, active-skill root injection, or persisted
canonical skill roots in this slice. Those belong to roadmap items `1.3.3` and
`1.3.4`.

Tests must avoid live network dependencies. For URL flows, use injected or
local test fetch seams where available, or test the adapter immediately below
the HTTP fetch if the existing API has no injection seam.

Use `rstest` fixtures for repeated archive, registry, and web-state setup.
Use `rstest-bdd` for behaviour coverage where it is practical and focused; if
the current test harness makes BDD disproportionate for this slice, record why
in `Surprises & Discoveries` and cover observable behaviour with `rstest`.

Run `make all` before any implementation commit. Run Markdown validation for
changed Markdown files with `bunx markdownlint-cli2 <paths>` and run
`git diff --check`.

## Tolerances

If the implementation appears to require changing more than one public install
endpoint path, stop and record the reason. The target is one logical web
install endpoint, not a parallel API family.

If multipart uploads require increasing the global gateway body limit above
the RFC bundle cap, stop and decide whether a route-local limit is possible.
The implementation should not silently widen all browser request limits.

If tool-call upload support appears to require base64 archives in the Large
Language Model (LLM) tool schema, stop and ask for confirmation. Roadmap item
`1.3.2` requires uploaded bundles for the web flow and `.skill` URLs; adding
base64 archives to `skill_install` may be a separate model-facing contract
change.

If exact-one-source validation would break an existing catalogue or inline
install workflow, stop and document the compatibility conflict before
continuing.

If new dependencies appear necessary for multipart parsing, archive sniffing,
or MIME detection, stop. Prefer existing Axum, tower, and ZIP dependencies
unless review explicitly approves more.

If the feature touches more than about 12 source files or 700 net new lines
before tests and docs, stop and reassess whether runtime bundle work from
`1.3.3` or `1.3.4` has leaked into this slice.

If no practical `rstest-bdd` seam exists without adding a broad new harness,
document that finding and proceed with focused `rstest` behavioural tests
rather than forcing ceremony.

## Risks

The highest risk is adapter divergence. The current URL and catalogue installs
already route downloaded bytes into the shared staged install path. Uploads
must do the same, or future fixes to bundle validation will apply to one path
but not another.

Request-size handling is a medium risk. `src/channels/web/CLAUDE.md` documents
a 1 MiB default gateway body limit, while RFC 0003 also caps bundle archives.
If those limits interact badly, the user may see a generic HTTP `413` before
the bundle validator can return a typed archive-shape error.

Error mapping is a medium risk. The existing `map_skill_install_error` treats
write errors as `500` and other registry errors as `400`. Multipart extraction
and source validation must preserve this clarity without hiding
`invalid_skill_bundle` details.

The `skill_install` tool schema is a compatibility risk. It currently requires
`name` even for non-catalogue installs. Making source alternatives explicit is
the right contract direction, but it must be tested against existing inline,
URL, and catalogue install behaviour.

The UI is simple static JavaScript. Adding a file picker and multipart request
must fit the existing browser style without turning the Skills tab into a
separate workflow or adding explanatory text that belongs in the users guide.

Behaviour-driven testing may be awkward because the existing skill tests are
mostly unit-style Rust tests and Python end-to-end tests. The mitigation is to
add the smallest useful `rstest-bdd` scenario only if it can run as ordinary
Rust test code without a new app-level server harness.

## Progress

- [x] 2026-04-24: Read the roadmap entry, RFC 0003, documentation index,
  welcome guide, architecture overview, users guide, skills design document,
  web gateway notes, relevant source seams, and the previous `1.3.1`
  ExecPlan.
- [x] 2026-04-24: Used a Wyvern agent for a parallel planning brief covering
  likely files, boundaries, docs, tests, and risks.
- [x] 2026-04-24: Drafted this approval-gated ExecPlan.
- [x] 2026-04-24: Plan approved by the user for implementation.
- [x] 2026-04-24: Implementation started. The first implementation action is
  to pin existing install contracts and add failing coverage for browser
  `.skill` upload support before changing handler behaviour.
- [x] 2026-04-24: Upload and `.skill` URL install behaviour implemented.
  The web install handler now accepts JSON or multipart input, enforces one
  install source before fetching or staging, and sends uploaded bundle bytes
  through an archive-only registry payload.
- [x] 2026-04-24: Unit and behavioural tests added with `rstest`. Coverage
  includes archive-only upload bytes, multipart upload preservation of
  `references/` and `assets/`, explicit archive-shape failures, inline JSON
  regression coverage, and exact-one-source failures for web and tool inputs.
- [x] 2026-04-24: Documentation and roadmap updated.
  `docs/users-guide.md`, `docs/agent-skills-support.md`,
  `docs/front-end-architecture.md`, `src/channels/web/CLAUDE.md`, and
  `docs/roadmap.md` now describe uploaded bundles, `.skill` URL installs,
  exact-one-source validation, and the remaining runtime-file-read limitation.
- [x] 2026-04-24: Full validation passed. The final full gate was
  `make all 2>&1 | tee /tmp/make-all-axinite-session-5cef00e2.out`, which
  completed formatting, linting, the default nextest workspace run, and the
  GitHub tool tests successfully.
- [x] 2026-04-24: Documentation gates passed. The final Markdown lint command
  wrote `/tmp/markdownlint-axinite-session-5cef00e2.out` with zero errors, and
  `git diff --check` wrote `/tmp/diff-check-axinite-session-5cef00e2.out`
  without whitespace findings.
- [x] 2026-04-24: Feature committed as
  `a8d63a1a Support skill bundle upload installs`.
- [x] 2026-04-24: Code-review feedback addressed. The web and tool adapters now
  share source blankness helpers, multipart source fields use the same
  whitespace semantics as JSON installs, body size failures distinguish
  Axum's length-limit error from other body read errors, and regression tests
  cover invalid multipart uploads, mixed multipart sources, JSON size limits,
  additional ambiguous tool-source combinations, and downloaded raw Markdown.
- [x] 2026-04-24: Review-fix validation passed. The targeted skill test run
  `cargo test -p ironclaw skill` wrote
  `/tmp/test-review-skill-comments-axinite-1-3-2-extend-skill-installation-flows-bundles.out`
  and passed. The full gate
  `make all 2>&1 | tee /tmp/make-all-review-comments-axinite-1-3-2-extend-skill-installation-flows-bundles.out`
  also passed.
- [x] 2026-04-26: Follow-up CI warnings addressed. Added focused
  `install_source` unit tests, a Node `node:test` suite for the browser bundle
  upload helpers, a full gateway multipart upload integration test, JSDoc for
  `installSkillBundleFromForm()`, module export documentation for
  `install_source`, and developer-guide notes for `ArchiveBytes`,
  `install_source`, `TestGatewayBuilder`, and the request-based web handler.
- [x] 2026-04-26: Follow-up validation passed. Targeted checks
  `cargo test -p ironclaw skill` and
  `node --test tests/web_static_app.test.mjs` passed, and the full gate
  `make all 2>&1 | tee /tmp/make-all-ci-warnings-axinite-1-3-2-extend-skill-installation-flows-bundles.out`
  passed with 3,989 nextest tests and the GitHub tool tests.

## Surprises & Discoveries

The request references `docs/axinite-architecture-summary.md`, but that file is
absent in this checkout. `docs/axinite-architecture-overview.md` is the
available high-level architecture document and already mentions the skills
startup phase and staged install path.

Roadmap item `1.3.1` is marked complete, and the source already contains
`src/skills/bundle/`, `SkillInstallPayload::DownloadedBytes`, and staged
bundle preservation tests. This means `1.3.2` should not rework validation; it
should extend input adapters and user-visible install flows.

`docs/users-guide.md` currently says local `.skill` uploads are not yet
supported. That statement becomes false when this plan is implemented and must
be updated in the same feature branch.

`docs/agent-skills-support.md` notes that the runtime still injects only
`SKILL.md` and has no dedicated bundled-file read tool. That limitation should
remain true after `1.3.2`.

2026-04-24T11:30:38Z: Enabling Axum's existing `multipart` feature pulled
`multer` and `spin` into `Cargo.lock`; no direct new crate dependency was added
to `Cargo.toml`.

2026-04-24T11:30:38Z: No existing skill-focused `rstest-bdd` harness is wired
into this repository. The proportional behavioural proof for this slice is the
in-process Axum handler coverage in `src/channels/web/handlers/skills/tests.rs`
using `rstest`, rather than adding the first BDD harness around the same
request/response behaviour.

2026-04-24: `make all` found that top-level `oneOf` is forbidden in the
repository's OpenAI tool-schema subset. The install tool therefore advertises
no globally required field and documents each source property, while runtime
validation enforces exactly one of `content`, `url`, or `name`.

2026-04-24: `make all` also found one recorded tool trace that still called
`skill_install` with both `name` and `content`. That trace represented the old
implicit catalogue-name requirement, so the fixture was updated to send only
`content` for an inline install.

2026-04-24: Axum's `to_bytes` error wraps `http_body_util::LengthLimitError`
as the source error when the configured body cap is exceeded. Matching that
source preserves the intended `413 Payload Too Large` response without
misclassifying unrelated body read failures.

2026-04-26: The static web app has no existing package manifest or frontend
test runner. The follow-up frontend coverage therefore uses Node's built-in
`node:test` runner and evaluates only the pure helper block from `app.js`,
avoiding a new JavaScript dependency chain.

## Decision Log

2026-04-24: Treat `1.3.2` as an install-adapter and user-interface slice over
the existing `1.3.1` validator. The reason is that RFC 0003 separates archive
validation and extraction from installation flows, and the roadmap marks
`1.3.1` complete.

2026-04-24: Keep the future `skill_read_file` tool and active-skill root
metadata out of this plan. They are explicitly covered by roadmap items
`1.3.3` and `1.3.4`, and mixing them into upload support would make approval
and validation harder.

2026-04-24: Plan for source-mode validation at the adapter boundary and archive
shape validation in `src/skills/`. This follows the hexagonal-architecture
guidance without forcing a broad directory restructure.

2026-04-24: Prefer a multipart upload path for the browser Skills page rather
than base64 in the JSON `SkillInstallRequest`. Multipart matches RFC 0003's
recommended upload shape and avoids inflating archives inside JSON.

2026-04-24T11:30:38Z: Add `SkillInstallPayload::ArchiveBytes` for uploaded
`.skill` files instead of reusing downloaded bytes. URL downloads must continue
to accept either raw `SKILL.md` or `.skill` archives, but browser upload is an
archive-only contract and should not silently install a renamed markdown file.

2026-04-24: Do not use top-level `oneOf` in `skill_install` even though it
would express source alternatives in JSON Schema. The local schema validator
for model-facing tools forbids that keyword at the top level, so the compatible
contract is an object with optional source properties plus explicit runtime
validation.

2026-04-24: Keep source-selection control flow local to each adapter, but share
the small string-normalisation helpers in `src/skills/install_source.rs`. This
avoids a larger enum abstraction while keeping web, multipart, and tool
definitions of blank source fields aligned.

2026-04-26: Use `TestGatewayBuilder::start()` for multipart upload integration
coverage. Handler-level Axum tests still provide detailed unhappy-path
coverage, while the integration test proves the complete authenticated HTTP
upload path writes the expected bundle files.

## Implementation plan

Milestone 1 establishes the current contract with tests before changing
behaviour. Inspect `SkillInstallPayload`, `materialize_install_artifact`,
`skills_install_handler`, and `SkillInstallTool::parameters_schema()`. Add or
adjust tests that pin existing inline, catalogue, and URL installs so later
request-shape changes cannot break them silently. The expected result is that
existing install paths still pass and there is a clear failing test for local
`.skill` upload support.

Milestone 2 extracts a shared install orchestration helper if the current code
requires it. Both `src/channels/web/handlers/skills.rs` and
`src/tools/builtin/skill_tools/install.rs` currently perform the same
prepare-commit-cleanup sequence. If adding uploads would duplicate more of
that flow, move the shared sequence into a small skills-owned application
service or helper that accepts an install root plus `SkillInstallPayload` and
returns the installed name. Keep registry locking brief and keep async disk I/O
outside long-held locks, as `docs/agent-skills-support.md` describes. If the
existing duplication remains small, leave it alone and only add shared
source-validation helpers.

Milestone 3 makes install sources explicit. Define a small internal source
selection function for the web and tool adapters that accepts the possible
inputs and returns exactly one mode. It should reject zero sources and multiple
sources with specific messages. For JSON installs, preserve current content,
URL, and catalogue behaviour. For direct URL installs, continue to call
`fetch_skill_bytes()` and then pass the bytes as
`SkillInstallPayload::DownloadedBytes`, so `.skill` URLs automatically use the
existing archive validator.

Milestone 4 adds the web upload adapter. Extend
`src/channels/web/handlers/skills.rs` so `POST /api/skills/install` can accept
either the existing JSON body or multipart form data with one `.skill` file
part. Preserve `X-Confirm-Action: true`. Read the uploaded file into bounded
bytes, reject missing or multiple source fields, and pass the bytes to the
shared staged install path as `SkillInstallPayload::ArchiveBytes`, preserving
the archive-only upload contract. Ensure an invalid archive root returns the
existing `invalid_skill_bundle: expected one top-level path prefix with
SKILL.md at <root>/SKILL.md` style message.

Milestone 5 updates the browser Skills tab. In
`src/channels/web/static/index.html`, add a file picker for `.skill` archives
near the existing URL install controls. In `src/channels/web/static/app.js`,
send multipart `FormData` for uploaded files and keep JSON for catalogue,
inline, or URL installs. In `src/channels/web/static/style.css`, keep the
layout consistent with the current compact operations UI. The first viewport
does not need a landing page or explanatory marketing text; the users guide
will document the file-format rules.

Milestone 6 adds focused tests. Use `rstest` fixtures to build valid and
invalid archives, registries with temporary installed directories, and any web
gateway state needed by the handler. Cover successful upload preserving
`references/` and `assets/`, upload rejection for malformed archive shape,
`.skill` URL install preserving ancillary files by exercising downloaded
bytes, exact-one-source validation, and regression coverage for plain
`SKILL.md` URL or content installs. Add a `rstest-bdd` scenario only if a
small in-process feature can exercise "Given a valid skill bundle, When it is
uploaded, Then its references and assets are installed" without starting a
full browser server. Otherwise record why `rstest` handler tests are the
proportional behavioural proof.

Milestone 7 updates documentation. In `docs/users-guide.md`, replace the
statement that local `.skill` uploads are not supported with the new URL and
upload workflow. Include the important user-visible failure rule: malformed
archives report explicit `invalid_skill_bundle` errors and failed installs do
not leave partial skill trees. In `docs/agent-skills-support.md`, update the
web API and management-surface sections to mention uploaded `.skill` bundles
and the exact-one-source install contract. In `docs/front-end-architecture.md`
or `src/channels/web/CLAUDE.md`, update the Skills route notes if the request
content type or body-limit behaviour changes. Check `FEATURE_PARITY.md` for
any status row that must change.

Milestone 8 validates and completes the roadmap item. Run targeted tests
first, then:

```bash
BRANCH_FOR_LOGS=$(git branch --show | tr / -)
make all 2>&1 | tee /tmp/make-all-axinite-${BRANCH_FOR_LOGS}.out
bunx markdownlint-cli2 \
  docs/execplans/1-3-2-extend-skill-installation-flows-bundles.md \
  docs/users-guide.md \
  docs/agent-skills-support.md \
  docs/front-end-architecture.md \
  docs/roadmap.md \
  src/channels/web/CLAUDE.md \
  2>&1 | tee /tmp/markdownlint-axinite-${BRANCH_FOR_LOGS}.out
git diff --check 2>&1 | tee /tmp/diff-check-axinite-${BRANCH_FOR_LOGS}.out
```

If those pass, mark roadmap item `1.3.2` done in `docs/roadmap.md`, update the
progress and retrospective sections of this ExecPlan, and commit the feature
with the `commit-message` skill.

## Validation plan

Before implementation, run the smallest relevant existing skill install tests
to establish a baseline. A likely command is:

```bash
BRANCH_FOR_LOGS=$(git branch --show | tr / -)
cargo test skill_install \
  2>&1 | tee /tmp/test-skill-install-baseline-axinite-${BRANCH_FOR_LOGS}.out
```

During implementation, run targeted tests after each milestone that changes
behaviour. Prefer narrower filters such as:

```bash
cargo test -p ironclaw skills::registry::tests \
  2>&1 | tee /tmp/test-skill-registry-axinite-${BRANCH_FOR_LOGS}.out
cargo test -p ironclaw channels::web::handlers::skills \
  2>&1 | tee /tmp/test-web-skills-axinite-${BRANCH_FOR_LOGS}.out
cargo test -p ironclaw tools::builtin::skill_tools \
  2>&1 | tee /tmp/test-skill-tools-axinite-${BRANCH_FOR_LOGS}.out
```

If exact module filters differ, use the closest `cargo test` filters shown by
`cargo test -- --list` and record the final commands in `Progress`.

The final gate before committing implementation is:

```bash
BRANCH_FOR_LOGS=$(git branch --show | tr / -)
make all 2>&1 | tee /tmp/make-all-axinite-${BRANCH_FOR_LOGS}.out
bunx markdownlint-cli2 <changed-markdown-files> \
  2>&1 | tee /tmp/markdownlint-axinite-${BRANCH_FOR_LOGS}.out
git diff --check 2>&1 | tee /tmp/diff-check-axinite-${BRANCH_FOR_LOGS}.out
```

Expected success is that each command exits with status `0`. For the feature
itself, success is observable when a valid uploaded `.skill` archive and a
valid HTTPS `.skill` URL install to the installed-skills directory with
`SKILL.md`, `references/...`, and `assets/...` present, and a malformed archive
fails with an explicit `invalid_skill_bundle` message.

## Outcomes & Retrospective

The implementation extends the existing staged skill installer rather than
duplicating bundle policy in the web adapter. HTTPS URL installs continue to
use the existing fetch and downloaded-bytes path, so raw `SKILL.md` URLs and
`.skill` archive URLs remain supported by the same SSRF-protected transport.
Browser uploads use a new archive-only payload variant, which means a renamed
Markdown file cannot accidentally be accepted as an uploaded bundle.

The web endpoint now accepts the existing JSON request shape or multipart form
data with a single `bundle` file field. Both the web adapter and the
model-facing `skill_install` tool reject missing or ambiguous source
combinations before install side effects. The tool schema cannot express this
with top-level `oneOf` because the repository's OpenAI-compatible schema
validator rejects that keyword, so the explicit contract is enforced at
runtime and documented.

Focused `rstest` coverage proved the relevant happy and unhappy paths:
uploaded bundles preserve `references/` and `assets/`, malformed uploads
return explicit archive-shape errors, inline JSON installs still work, and
ambiguous source combinations fail. No proportional `rstest-bdd` harness
already existed for skill install flows, so this slice used in-process Axum
behaviour tests rather than adding a new BDD layer for the same request and
response proof.
