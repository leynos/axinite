# Typed setup fields and delegated endpoint binding persistence (2.1.1)

Status: DRAFT — awaiting explicit approval before implementation.

This execution plan (ExecPlan) is a living document. Keep its progress,
decisions, verification evidence, and conformance links current at every
milestone. This branch initially delivers planning documents only.

## Purpose and observable outcome

A user can configure a confidential endpoint in extension setup alongside an
API credential. The credential remains in `SecretsStore`; the endpoint goes
through a dedicated validation service into an encrypted, user- and
extension-scoped binding store. Reloading setup shows configuration status,
never the saved URL. Replace and clear actions work without revealing it.
PostgreSQL and libSQL provide the same behaviour across process restarts.

This implements roadmap **2.1.1**, the first phase of RFC 0004. It does not
implement delegated network execution, WIT calls, credential injection by
endpoint identity, request approvals, or a JMAP pilot. Those belong to
2.1.2–2.1.4. Here, configured means setup validation succeeded; it neither
grants transport authority nor proves that a remote service is reachable.

## Conformance basis

The inspected baseline is commit `9290dbc191d928cab6182904582491fc35dfb33e`.
Governing sources are:

- [Roadmap 2.1](../roadmap.md#21-delegated-endpoint-requests).
- [RFC 0004](../rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md).
- [Architecture overview](../axinite-architecture-overview.md).
- [Proposed ADR 014](../adr-014-confidential-endpoint-setup.md).

RFC 0004 remains Proposed; approval of this plan accepts only the scoped
choices below, not all later RFC phases. There is no separate
terms-of-reference document.

The selective trace identifiers are:

- **R1:** RFC typed fields and roadmap success: declared values go to their
  correct store; existing packaged secret-only setup remains usable.
- **R2:** RFC binding store: scope, encryption, validation before persistence,
  replacement, clearing, and both backend implementations.
- **R3:** RFC setup UI: status-only reads and useful, confidential failures.
- **R4:** Phase objective: preserve capability, authentication, and approval
  boundaries; no new guest or model endpoint-discovery surface.
- **R5:** Delivery: substantive proof for the selected pure kernel, executable
  integration tests, documentation, and honest roadmap completion.

R1 maps to M1/M3 and A1/A3; R2 to M2 and A2; R3 to M3/M4 and A3/A4; R4 to every
milestone and A2/A4; R5 to M1/M5 and A1/A5. Acceptance identifiers are defined
below. Roadmap 1.2 is the prerequisite; confirm its completed contract tests
remain passing before final delivery.

## Repository orientation and required reading

Begin with `AGENTS.md` and these orientation documents:

- [Contents](../contents.md).
- [Product direction](../welcome-to-axinite.md).
- [Repository layout](../repository-layout.md).
- [Architecture overview](../axinite-architecture-overview.md).

Read `src/tools/README.md`, `src/channels/web/CLAUDE.md`, `src/db/CLAUDE.md`,
`src/setup/README.md`, and `tests/e2e/CLAUDE.md` before modifying their
surfaces. The main seams at the inspected baseline are:

- `src/tools/wasm/capabilities_schema/auth.rs`: `ToolSetupSchema` currently
  exposes `required_secrets`. Its parsing tests are under
  `src/tools/wasm/capabilities_schema/tests/`.
- `src/channels/wasm/schema/setup.rs`: channel `SetupSchema`, including
  legacy secret auto-generation. Channels remain secret-only in scope, but must
  keep working through the shared setup API.
- `src/extensions/manager/setup_secrets.rs`: `get_setup_schema` and
  `save_setup_secrets` load manifests, write credentials, and activate tools.
  `src/extensions/manager/channel_state.rs` owns `check_tool_auth_status` and
  `setup_secrets_state`; token and setup readiness are currently coupled.
- `src/channels/web/types/extensions.rs` and
  `src/channels/web/handlers/extensions/setup.rs`: setup request/response types
  and handlers, including server-sent events (SSE).
- `web-src/axinite/src/lib/api/contracts.ts` and
  `web-src/axinite/src/components/extensions-preview.tsx`: the default Solid
  interface. `src/channels/web/static/app.js` is a supported legacy interface,
  selected by `AXINITE_WEB_UI=legacy`; both need the new contract.
- `src/db/traits/`, `src/db/forwarders.rs`, `src/db/postgres.rs`,
  `src/db/libsql/`, `src/history/`, `migrations/`,
  `src/db/libsql_migrations.rs`, and `migrations/libsql_schema.sql`: storage
  ports, adapters, and schema versions. Read current migration allocation
  before choosing the next version; do not edit an applied migration.
- `src/secrets/crypto.rs`: existing authenticated encryption primitive.
  Its current encrypt/decrypt API does not authenticate row identity.

Use the `codegraph-mcp`, `hexagonal-architecture`, and `rust-router` skills.
Route proof work through `rust-verification` and `verus`; use `firecrawl-mcp`
for primary-source tooling or protocol gaps. CodeGraph was unavailable in this
planning session; direct reads and `rg` were used as fallback. Agent teams
exchange excerpts through `context_pack`, but this document is the persistent
handoff and does not require those temporary packs.

The testing references are:

- [rstest fixtures](../rust-testing-with-rstest-fixtures.md).
- [Dependency injection](../reliable-testing-in-rust-via-dependency-injection.md).
- [Doctests](../rust-doctest-dry-guide.md).
- [Complexity and refactoring](../complexity-antipatterns-and-refactoring-strategies.md).
- [rstest-bdd](../rstest-bdd-users-guide.md).
- [Formal verification](../formal-verification-methods-in-axinite.md).

Use fallible fixtures, injected clocks/randomness/stores, and no process-wide
environment mutation. Keep new modules below 400 lines and isolate policy from
orchestration rather than adding branches to the existing manager.

## Behavioural contracts before implementation

**C1 — typed declaration and routing.** A validated schema has unique field
keys. A secret field key is its credential name; an endpoint field key is
exactly its opaque `endpoint_name`, with a separate `endpoint_kind`. Normalized
destination identities must be unique, including legacy secret declarations;
two fields cannot alias the same stored value. Multiple named endpoints per
extension are supported. The authenticated host context supplies user and
installed extension identity; neither comes from submitted values. Reject
undeclared keys, duplicate keys (including duplicate JSON object members),
unknown tags, wrong-kind values, and malformed declarations before any write.
Never infer kind from a string that happens to resemble a URL.

Preserve deserialization of shipped `setup.required_secrets` manifests by
normalizing them at the manifest boundary into secret fields. If both legacy
and new non-empty field declarations are supplied, reject the manifest rather
than silently merging conflicting definitions. This compatibility serves
existing installed artefacts. Update application-internal callers and both
bundled UIs together; do not add deprecated source API wrappers or a second
secret-only runtime implementation.

**C2 — validation and storage.** Endpoint input must parse as an absolute HTTPS
URL with a host and no userinfo, fragment, or query. Query credentials belong
in credential configuration, not endpoint setup. Reject control characters,
backslashes, local-only hostnames, and non-public IP literals, including
alternate numeric spellings after parsing. Preserve the parsed path semantics
and use one canonical serialization; do not hand-write a second URL parser.
Canonicalization must be idempotent for accepted inputs.

Setup validation is offline: do not fetch, resolve DNS, follow redirects, or
contact the supplied endpoint. Domain-name syntax checks cannot rule out
private DNS answers or rebinding; later transport must resolve and authorize
addresses at connection time. Endpoint kind is declarative metadata here;
`jmap` does not trigger discovery or provider HTTP checks. A record can be
configured without being reachable. Record a validation-policy version so
unknown or stale versions cannot silently count as valid after policy changes.

The store key is `(user_id, extension_name, endpoint_name)`. Values contain
versioned authenticated ciphertext, salt/nonce as required by the existing
crypto primitive, endpoint kind, validation-policy version, and timestamps. Do
not persist a plaintext origin, URL, path, or free-form error. Authenticate the
entire scope, endpoint kind, validation-policy version, and envelope version
using unambiguous length-delimited associated data and endpoint-specific domain
separation. Keep existing secret ciphertext readable. Decryption, unknown
envelope versions, and corrupted records fail closed with a safe error category.

**C3 — mutation and failure semantics.** Omitted fields mean keep; endpoint
`set` means validate then replace; endpoint `clear` means remove and is
idempotent. Empty endpoint strings are invalid, not implicit clear. Retain
legacy empty-secret keep behaviour in the manifest-normalized secret path. A
missing required endpoint is `needs_input`; backend/decryption failure is
`error`, not an absent record. Optional absent fields do not block readiness.
Any declared field with a read/authentication error blocks aggregate readiness,
including an optional field. An authenticated record with a stale policy
version or a kind different from its current declaration is invalid and needs
attention; replace or clear it explicitly. Do not silently revalidate.
Undeclared rows are retained for rollback but never surfaced; reintroduced
names require kind/policy checks. Successful extension removal deletes that
scope through the service; removal failure is reported and retried explicitly,
not silently ignored.

Validate the complete submission before effects. A validation failure leaves
both stores unchanged. Persistence across the credential store and endpoint
store is not a distributed transaction: after validation, apply operations in a
deterministic order and return safe per-field outcomes on an I/O failure.
Distinguish `committed`, `unchanged`, `indeterminate`, and `unattempted`
per-field outcomes. A lost acknowledgement after a possible commit is
indeterminate; never claim unchanged without confirmation. Previously committed
fields may remain; unattempted fields are identified, no success/activation
event is emitted, and the UI refreshes authoritative status. Never attempt
unsafe compensating writes that might overwrite a concurrent save. Retrying a
set has the same logical result only without intervening writes; timestamps and
random ciphertext may change. Retrying after uncertainty is an explicit user
action which may overwrite another administrator. Do not replay mutations
automatically. Status refresh confirms availability, not which concurrent
replacement won. Each endpoint replace/clear is atomic, with last database
commit winning; do not promise compare-and-swap semantics.

**C4 — confidentiality and readiness.** GET, submit responses, errors, SSE,
tracing, audit metadata, tool catalogues, and model-visible output carry only
declared identifiers and status. Do not derive `Debug` or `Serialize` that
exposes submitted URLs or decrypted values. Do not log raw request bodies,
parser errors, ciphertext, or backend error chains on this path. Unknown
submitted keys must not be reflected because they may themselves contain a URL.
UI input is cleared after submission and never stored in browser storage. The
user necessarily sees their own entered value; the server never returns it.

Readiness requires no declared-field error/invalid state, all effective
required setup fields available, and the existing authentication requirements.
Effective credential availability preserves existing token-authoritative auth
and approved auto-resolved intermediate OAuth fields; do not newly require
intermediate client credentials when the existing auth contract does not.
Endpoint requirements are always checked, including tools with an auth section.
A valid OAuth token cannot bypass a missing required endpoint. Conversely,
endpoint setup does not satisfy OAuth. An endpoint-only edit must not purge
OAuth tokens or start reauthentication. After clear, recompute readiness before
activation; future delegated use must always check current bindings, never
cached setup success. Existing raw-URL HTTP grants, WIT contracts, and
approvals remain untouched.

## Abstract model and proof sketch

Define the model before executable structures. Let `D` be a finite sequence of
declarations with unique keys and kind `Secret` or `Endpoint`. Let `V` be a
finite sequence of submitted operations, preserving duplicates until they are
rejected. Let `S` map scoped endpoint keys to validated confidential values;
credential availability is a separate abstract input. Payloads are opaque
atoms: no proof reasons about cryptography or URL syntax.

`plan(D, V, scope)` returns either a rejection or two disjoint sequences of
credential and endpoint operations. Its specification is relational: each
accepted submitted operation has exactly one matching declaration and one
output in the matching destination, with the host scope unchanged. Rejection
produces no executable operations. `apply(S, op)` replaces or removes only that
operation's exact scoped key. `summary(D, availability, auth)` exposes only
identifiers, categories, and readiness, never payload atoms.

Preconditions for accepted operations are a valid declaration set, unique
submitted keys, matching variants, and successful endpoint validation evidence.
Malformed inputs are part of the total rejection contract, not excluded from
all reasoning by a precondition. Postconditions include preservation of other
keys and exact readiness over all required declarations. The representation
relation maps Rust validated identifiers to model keys, vectors to sequences,
DB rows to scoped entries, and encrypted/decrypted values to the same opaque
atom only after authentication succeeds. A missing row and a store failure must
remain distinct in this relation. Declaration-to-storage mapping is injective;
endpoint field keys equal endpoint names and secret keys equal credential
names. The model includes missing, valid, stale, and error states, rather than
collapsing availability to a boolean.

The initial semantic proof hypotheses are:

- **P1 partition and conservation:** for arbitrary-length accepted inputs,
  every operation occurs exactly once in the correct output and no invented
  operation occurs; permutations of unique input operations have equivalent
  effects. Wrong variants or unknown/duplicate keys reject the whole plan.
- **P2 scope preservation:** a finite sequence of accepted operations changes
  only the designated scoped keys; identical endpoint names under other users
  or extensions remain unchanged. Clear is idempotent; replacement preserves
  every non-target value.
- **P3 projection and readiness:** equal declarations, availability, and auth
  state (including field error/invalid states) yield equal public summaries
  when only confidential payloads differ; readiness holds exactly when no
  declared field is invalid or errored, every effective required field is
  available, and existing auth requirements hold. This proves payload
  independence of the projection, not timing non-interference or arbitrary
  system-wide information-flow safety.

These are proposed semantic lemmas, not a prescribed solver decomposition.
Refine them if real verification conditions demand it. Do not introduce
`assume`, `admit`, external/trusted proof bodies, or an axiom equivalent to a
conclusion. Keep unproved statements here until the proof milestone is ready.
Do not prove URL parsers, AES, SQL, or Tokio from first principles.

Select Verus for P1–P3 because they apply to arbitrary sequence lengths and
scope identities in a small pure kernel. Prefer a shared executable kernel used
by production and the proof harness. If Verus cannot compile the exact
production representation, use the repository's documented separate proof model
with an explicit refinement relation, exhaustive finite tag truth tables,
source-drift checks, and generated differential tests. Record exact production
functions corresponding to P1–P3 in `verus/README.md`; every change to mapped
functions must update/review the representation mapping and differential
properties. Bridge CI must fail when mapped sources change without the bridge
record being updated. A refreshed hash alone is not refinement evidence. Label
this as a model proof with a tested bridge, not formal verification of all
production Rust. Escalate if the bridge cannot be made reviewable; do not
silently downgrade the promised guarantee. No proof infrastructure is
implemented in this PR.

## Intended design and interfaces

Create a cohesive `src/extensions/setup/` feature module, with `model.rs`,
`policy.rs`, `service.rs`, and small tests as needed. Keep domain policy pure:
no Axum, SQL, `reqwest`, filesystem, or encryption imports in `policy.rs`. The
feature owns `SetupField`, `SetupOperation`, `SetupState`, `BindingKey`,
`EndpointName`, and semantic errors. Exact filenames may adapt within this
ownership boundary; update the plan as they do.

The driving service accepts host scope and typed operations and returns a safe
setup outcome. Its driven `EndpointBindingRepository` port reads ciphertext
records, atomically replaces/deletes one scoped record, and reads scoped
encrypted records in a bounded batch. Computing configured status authenticates
each current declared record and checks its kind/policy against the
declaration; metadata existence alone is never enough. Bound batch work by the
manifest limit and do not cache decrypted URL values between requests.
`EndpointBindingService` owns validation, crypto use, and status; repository
adapters perform persistence, not policy. Keep plaintext resolution private to
the service until the host execution consumer in 2.1.2 exists. Do not add a
guest-accessible getter.

Use a narrow store interface consistent with ADR 006's native/dyn pattern and
the existing database factory; avoid widening every `Database` consumer if a
separately injected adapter suffices. Define the port in the feature, implement
PostgreSQL and libSQL adapters under `src/db/`, and construct them in an
owning-module factory. `ExtensionManager` orchestrates credential and endpoint
services; web handlers only decode input and project safe results. Backend
feature branching belongs in the factory, not `app.rs`.

Typed request values use explicit tagged variants, for example secret value,
endpoint set, and endpoint clear. Declarations and values are distinct types.
Response fields include safe labels, kind, optionality, and provided/configured
status. Errors use a semantic category plus a declared field identity where
available; no arbitrary source text. Keep existing auth setup URLs separate:
they must never be populated from a confidential endpoint value.

In the UI, render secret controls as passwords and endpoint controls as URL
inputs. Show the RFC's host-only explanation, Configured/Needs attention, and
explicit replace/clear actions. Keep blank edits from clearing values. Update
both Solid and legacy UIs, their API mocks, generated embedded assets, and
error handling together. The limited compatibility decision concerns packaged
manifests, not obsolete copies of application-internal DTOs.

## Milestones

### M1 — freeze contracts and establish the verified policy core

After approval, add failing `rstest` cases for C1/C3, then implement schema
normalization and a pure operation planner with all callers coherent. Add
property tests before proof engineering; they pressure-test the specification.
Create `verus/endpoint_setup.rs` with the model and substantive proofs P1–P3.
Pin Verus and the `rust-prover-tools` runner using `tools/verus/VERSION`,
`tools/verus/SHA256SUMS`, and `tools/rust-prover-tools/REF`; add a `make verus`
target and a CI lane for the selected proof. No successful skip or placeholder
proof target is acceptable as milestone evidence.

Acceptance A1: invalid inputs produce no operations, both kinds route
correctly, required/optional/auth cases have witnesses, and Verus reports zero
verification errors with the intended theorem count recorded. Deliberately
misroute an endpoint or omit a scope component in a disposable patch and show
proof/test rejection, then restore and verify. Check exact production-to-model
mapping and no trusted shortcuts. Run all ordinary gates and `make verus`.
Commit a coherent domain-only plateau with no new setup behaviour advertised.
If extraction needs an unrelated architecture rewrite, stop for a deviation.

### M2 — validate, encrypt, and persist through both adapters

Add failing shared store-contract tests, crypto tamper tests, and URL
validation cases. Implement the service, scoped adapters, and additive
migrations. Use unique composite keys in both databases and atomic
upsert/delete operations; select the next available PostgreSQL and libSQL
migration versions. Exercise upgrade from the previous schema, repeated
migration, reopen, and fresh creation. Do not put URLs in generic settings or
credential rows.

Acceptance A2: save/reload/replace/clear work on real PostgreSQL and
file-backed libSQL; a second user/extension cannot observe or mutate the first
binding; wrong keys, corrupted ciphertext, tampered validation-policy versions,
and transplanted rows fail safely, including on status GET. Rejected
replacement leaves the previous binding intact. Unknown validation versions are
not ready; upgrading a manifest kind requires replacement or clear. Test
removed/reintroduced declarations and explicit uninstall cleanup. Concurrent
replace/clear tests agree with a legal commit order. Inspect raw rows and
captured logs for sentinel hostname/path/query leakage. Run ordinary gates,
`make verus`, and `make test-matrix NEXTEST_PROFILE=ci`. Commit the internal
service plateau; no transport capability is added.

### M3 — integrate setup, web contracts, and both interfaces atomically

Start with failing manager and HTTP integration tests. Replace secret-only
setup coordination with the typed service. Validate every field before writes;
model partial I/O failure explicitly and recompute status from stores. Preserve
channel secret auto-generation, token setup/validation, and activation where
appropriate. Gate activation on both authentication and setup state. Endpoint
edits must not route through unconditional OAuth-token purge. Capture HTTP
extraction, manager, DB, and SSE failures to check safe output categories.

Acceptance A3: mixed submissions route correctly; malformed batches have no
effects; store failures report partial outcomes without claiming success; lost
write acknowledgements report indeterminate outcomes; required endpoint absence
blocks readiness even with OAuth; approved auto-resolved intermediate
credentials keep working; optional corrupt fields block readiness;
endpoint-only replacement preserves OAuth. Secret-only channel/tool regression
suites pass. Complete the UI work below in this same milestone before running
ordinary gates and `make verus`. Commit the API, both clients, generated
assets, and fixtures together; no committed plateau may break either bundled
client.

#### M3 interface work

Write failing frontend and browser tests for URL/password controls, no-echo
reload, replacement, explicit clear, and sanitized partial-failure recovery.
Update Solid contracts/components and legacy JavaScript together, then build
embedded assets through the Makefile. Use a test-only synthetic manifest with
one secret and two endpoints; do not ship a non-working JMAP extension.

The M3 UI acceptance includes correct controls, replace/clear, and safe error
recovery in both interfaces. Run `make frontend-check`, `make frontend-test`,
`make frontend-build`, and `make frontend-verify` sequentially, then ordinary
gates and `make verus`. A build failure does not justify hand-editing output.

### M4 — demonstrate end-to-end behaviour and recovery

Acceptance A4: an authenticated user submits mixed setup, reloads after a
server restart, sees Configured without the sentinel URL, replaces, clears, and
sees Needs attention. A second user cannot access the first binding. Inspect
responses, SSE, DOM, browser storage, server logs, and tool metadata for the
sentinel. No DNS/HTTP request to the configured endpoint occurs. Exercise
default and `AXINITE_WEB_UI=legacy` separately with child-process
configuration. Use `agent-browser` after loading its skill and running
`agent-browser skills get core`; use `css-view` for computed-style inspection
when checking the changed controls. Automated browser tests remain required.

Add `tests/e2e/scenarios/test_endpoint_setup.py` using real setup handlers and
persistent storage, not intercepted mock responses. From `tests/e2e`, run
`pytest scenarios/test_endpoint_setup.py -v`, with both interface variants
parameterized by fixtures. Extend fixtures to restart the same host/database
and to launch independent authenticated host contexts for scope tests; do not
invent multi-user identity headers. Run ordinary gates and `make verus`, then
commit the system-level proof of behaviour. Record exact commands and results.

### M5 — reconcile documentation and completion evidence

Update `docs/users-guide.md` for setup, no-echo replacement/clear, validation
limits, partial-save recovery, and credential separation. Update
`docs/developers-guide.md` for tagged DTOs, test fixtures, proof commands,
refinement maintenance, and re-use of the service by future endpoint kinds.
Update `docs/axinite-architecture-overview.md`, `src/tools/README.md`,
`src/db/CLAUDE.md`, `src/channels/web/CLAUDE.md`, and the accepted ADR with
actual ownership and proof-influenced invariants. Update the formal methods
document's old wrapper-script advice to the pinned runner actually adopted.
Check `FEATURE_PARITY.md` and update any affected status/notes in this branch;
never imply delegated requests already work. Add a precise entry if needed.

Acceptance A5: run the final ordinary, proof, backend-matrix, frontend, and E2E
gates on the final implementation candidate. Check conformance R1–R5 and record
evidence links, remaining limitations, and deviations. Only when all feature
acceptance conditions hold mark roadmap **2.1.1 done** using its checked-box
convention. Leave 2.1.2–2.1.4 unchanged. Commit gated documentation and final
evidence; do not mark completion for a plan or for partial tests.

## Verification plan

A1/P1–P3 use Verus over the pure model/kernel plus `proptest` refinement
checks. Generate declarations and histories by construction, not rejection
filters; record coverage for empty input, mixed kinds, duplicates, missing
required values, optional values, every operation, and distinct scopes. Use
non-empty accepted witnesses as well as rejection witnesses. Differential
checks compare independently specified effects, not a second copy of the
production loop. Saved failing seeds become named regressions. Kani adds no
needed evidence here and is not planned for async or crypto code.

A2/C2 use parameterized `rstest` validation tables and `proptest` accepted-URL
canonicalization idempotence. Include HTTPS hosts, ports, internationalized
names, IPv4/IPv6, numeric IPv4 aliases, empty/oversized inputs, credentials,
fragments, queries, control characters, and malformed authorities. Bound
requests by the existing gateway body limit and add explicit schema/value
length and field-count limits documented with tests at limit and limit plus
one. Initial limits: 128 fields/operations, 128 UTF-8 bytes per key, 64 bytes
per endpoint kind, and 8192 bytes per endpoint URL. Enforce these before
expensive parsing/encryption and retain stricter existing gateway limits. Test
parsed IP classification rather than text prefix matching.

A2/C3 also use shared backend contract tests and injected write/crypto
failures. Test persistence after restart, isolation, upsert uniqueness,
idempotent clear, concurrent operations, and cancellation before/after commit.
Both databases must execute tests; an unavailable service is a blocker, not a
skip. Cryptographic randomness, authenticated-encryption correctness, SQL
atomicity, gateway identity, the URL parser, and Rust/Verus toolchain soundness
are external assumptions. Test the repository's calls to these interfaces; the
proofs do not establish those libraries' internals.

A3/A4 use `rstest-bdd` scenarios at `tests/features/endpoint_setup.feature`,
bound from a focused Rust test module. Use shared fallible `rstest` fixtures.
Use `googletest::assert_that!` for rich matchers and `pretty_assertions` for
structured comparisons; the former does not require a non-fatal assertion
runner. Existing dependencies already include `rstest`, `rstest-bdd`,
`pretty_assertions`, `insta`, and `proptest`; add a SemVer-compatible dev
dependency on `googletest` during implementation. Do not upgrade unrelated
dependencies.

The minimum behavioural specification is:

```gherkin
Feature: Confidential extension endpoint setup
  Scenario: Configure and reload mixed fields
    Given an installed tool declares a secret and a required endpoint
    When its user saves valid values and restarts the host
    Then the endpoint is configured and the credential remains separate
    And setup responses contain no endpoint URL

  Scenario: Reject a malformed replacement without effects
    Given a configured endpoint and credential
    When a mixed submission contains an insecure endpoint URL
    Then neither stored value changes
    And the error identifies a safe validation category

  Scenario: Clear a required endpoint
    Given a configured required endpoint and a valid OAuth token
    When its user explicitly clears the endpoint twice
    Then setup needs input and the OAuth token is unchanged

  Scenario: Preserve scope isolation
    Given two users have the same extension and endpoint name
    When the first user replaces their endpoint
    Then the second user's configuration is unchanged

  Scenario: Recover from a partial save
    Given a valid mixed submission and an injected second-store failure
    When the user saves the submission
    Then the response reports committed and unattempted fields safely
    And no successful activation event is emitted

  Scenario: Lose a write acknowledgement
    Given a valid endpoint replacement
    When the database commits but its acknowledgement is lost
    Then the outcome is indeterminate
    And recovery offers explicit replacement without claiming no write occurred
```

Add unhappy and edge scenarios for unknown tags, duplicate keys, missing
required fields, corrupt records, missing crypto configuration, and stale
validation versions. `insta` snapshots cover all typed GET/submit/error/SSE
variants using synthetic data, paired with explicit no-sentinel assertions.
Never accept snapshots containing a real endpoint or credential. Snapshots
check output consistency; they do not substitute for the confidentiality
contract. A seeded fault that serializes the URL must fail the leak assertions.

## Concrete commands and evidence discipline

Run from the repository root. For each milestone, first run the new focused
test and record its expected failure before implementing the change, then
record its passing result. Proposed focused invocations, once targets exist:

```bash
cargo nextest run --workspace -E 'test(endpoint_setup)'
cargo nextest run --workspace --no-default-features \
  --features libsql-test-helpers -E 'test(endpoint_setup)'
prover-tools verus install --repo-root .
prover-tools verus run --repo-root . --proof-file verus/endpoint_setup.rs
```

Introduce Makefile aliases for repeatable feature and proof checks rather than
maintaining shell wrappers. At each major milestone run the repository
aggregate sequentially, never with `-j`; current `make all` runs
`make check-fmt`, `make lint`, `make test`, and `make spelling`. Run formatting
before those gates. The gate runner uses `set -o pipefail` and `tee`,
preserving the command status. For example:

```bash
set -o pipefail
make all 2>&1 | tee /tmp/all-axinite-2-1-1.out
make verus 2>&1 | tee /tmp/verus-axinite-2-1-1.out
make test-matrix NEXTEST_PROFILE=ci 2>&1 | tee /tmp/matrix-axinite-2-1-1.out
make markdownlint 2>&1 | tee /tmp/markdownlint-axinite-2-1-1.out
make nixie 2>&1 | tee /tmp/nixie-axinite-2-1-1.out
git diff --check
```

Execute these as separate sequential commands and stop on failure; the block is
a command inventory, not permission to continue after a failed command. Run
`make audit` for manifest, lockfile, or proof supply-chain changes. Run Rustdoc
tests explicitly (`cargo test --doc` and the libSQL-only equivalent) when
adding API examples because nextest does not run doctests. New Verus
infrastructure must preserve non-zero exits and verify pinned checksums; test
its negative paths. A successful proof run must report actual verified
obligations, never zero selected proofs.

Delegate full candidate gates to `scrutineer`; no other agent runs competing
full gates. Respect shared Cargo cache and compiler admission. Do not override
`RUSTC_WRAPPER`/`RUSTDOC`, place build targets in `/tmp`, kill another build,
or repair host services. Log capacity/admission failure separately from source
failure and report disk exhaustion immediately. Gate every commit and commit a
coherent logical change; failed red tests are evidence, not commits to ship.

For this documentation-only planning delivery, run `make markdownlint`,
`make nixie`, and `git diff --check`; no feature or Verus proof result is
claimed. Publish the requested branch over verified SSH, refresh its tracking
ref, and open a draft PR with title prefix `Plan:` and `(2.1.1)`. Link this
plan using the branch ref and put the Lody session link in the final
`## References` section of the PR body. Await explicit plan approval.

## Constraints

Preserve both databases, all existing secret-only packaged extensions, and both
supported web interfaces. Do not store endpoints as credentials or plaintext
settings. Do not expose stored endpoints through tool/model APIs. Do not widen
raw HTTP permissions or conflate setup validity with permission to connect. No
implementation begins before approval. No proof shortcuts, lint suppressions,
fake passing gates, or roadmap completion during planning.

## Tolerances (exception triggers)

Ordinary function splitting, test placement, and migration number allocation
are permitted within the approved ownership boundaries. Stop and record a
BLOCKED deviation if a change requires a new transport capability, breaking a
deployed manifest contract, cross-extension sharing, a global persistence
refactor, a new non-test runtime dependency beyond the agreed crypto reuse, or
weakening a stated contract. Likewise stop if formal model/production
refinement cannot substantiate the selected guarantee. Describe options and
upstream impacts before requesting approval; silence is not approval.

## Risks

The largest risks are accidental raw-value logging, OAuth-token deletion from
reusing the old save path, and overstating offline validation. Typed safe
results, an endpoint-only mutation branch, and explicit status semantics
address these. Multi-store partial failure is deliberately visible rather than
hidden behind an unrealizable transaction. Concurrent last-commit-wins updates
can overwrite another administrator's edit; document that limitation.

Encryption protects database contents, not a compromised host, user-entered
browser values, or memory dumps. Logical deletion does not securely erase old
WAL pages/backups. Preserve key availability for restore and rollback; key
rotation is not introduced here. Metadata identifiers remain visible, so they
must be manifest-defined opaque names, not copied hostnames.

Verus tooling is new here and may expose representation constraints. M1 settles
the refinement boundary before storage and UI work. Compiler posture is unknown
at the baseline (no root toolchain file was found); inspect CI, wrapper flags,
and the Rust router's canary before making borrow-sensitive API workarounds.
The plan does not rely on a particular borrow checker.

## Idempotence and recovery

Before each milestone inspect Git state and retain unrelated edits. Additive
migrations must be safe to rerun; never drop tables as an automated rollback.
Back up persistent data and master-key material through existing operator
procedures before deployment. Application rollback leaves the new table unused;
preserve it for roll-forward and avoid reverting encryption formats without a
data migration. Retry interrupted tests against disposable fixtures, not real
user bindings. After uncertain save completion, reload status and retry
explicit operations; never imply that an error proves no commit occurred.

## Progress

- [x] 2026-10-20: Inspected baseline, RFC, roadmap, and module boundaries.
- [x] 2026-10-20: Renamed the branch and completed Wyvern reconnaissance.
- [x] 2026-10-20: Drafted contracts, proof sketch, milestones, and proposed ADR.
- [x] 2026-10-20: Six expert perspectives reviewed and revisions reconciled.
- [x] 2026-10-20: `make markdownlint` passed its spelling prerequisite;
  generated `typos.toml` has no tracked diff. Direct candidate lint is required
  because this target selects committed changes only.
- [x] 2026-10-20: Nixie and diff whitespace checks passed. Direct Markdown
  lint found three overlong reference lists; split them before final checks.
  Publication and final candidate gate evidence are recorded in the draft PR.
- [ ] Explicit user approval obtained.
- [ ] M1 verified policy core.
- [ ] M2 encrypted dual-backend persistence.
- [ ] M3 setup service, web contracts, and both interfaces.
- [ ] M4 E2E and recovery evidence.
- [ ] M5 documentation, final evidence, and roadmap completion.

## Surprises & discoveries

The supported legacy UI is still live; updating only Solid would break setup.
The current Solid setup text input also needs correct secret/password typing.
`save_setup_secrets` can purge OAuth tokens after activation, so endpoint-only
updates cannot reuse that flow unchanged. Existing secret encryption has no
associated-data scope binding. `make all` includes spelling beyond the three
Rust gates described in some guidance. No existing Verus proof tree was found.

## Decision log

2026-10-20: Choose a dedicated encrypted endpoint store with scope-bound
authentication; reuse crypto mechanics without assigning credential semantics
to endpoint configuration. See proposed ADR 014.

2026-10-20: Define offline structural validation and defer connection-time
network policy to 2.1.2. This avoids introducing an unsafe setup-time HTTP
client and makes the meaning of Configured explicit.

2026-10-20: Select pure routing, scoped updates, and safe projection as proof
obligations. Database, crypto, parser, and UI integration remain independently
tested. Proof statements here are hypotheses, not pre-approved solver code.

2026-10-20: Preserve deployed manifest formats, migrate internal DTO consumers
together, and expose partial I/O outcomes instead of pretending the separate
credential and endpoint stores share a transaction.

2026-10-20 expert revision: Pandalump and Wafflecat identified aliased
destinations and an unhealthy API/UI milestone split. Unify field/destination
identity and make the cutover one gated M3 commit. Telefono required policy
version authentication and declaration-evolution checks. Doggylump and Buzzy
Bee required indeterminate write outcomes, authenticated status reads, bounded
work, and explicit retry semantics. Dinolump required a concrete proof-bridge
maintenance contract. Preserve the trade-off: a narrow store and last-commit
wins remain simpler than a vault or distributed transaction coordinator, with
uncertainty and concurrent overwrite made explicit.

## Outcomes & retrospective

The review-ready plan and proposed ADR capture the requested contracts, proof
sketch, implementation sequence, tests, and documentation obligations. Wyvern
reconnaissance and all six expert perspectives are complete; follow-up review
confirmed their findings resolved. Publication evidence belongs in the draft
PR, including direct candidate Markdown lint because the Makefile target only
selects committed changes. No implementation, migration, proof, or user-visible
feature has been delivered. The implementation status remains pending explicit
approval, and roadmap 2.1.1 remains unchecked.

## Research references

Primary references consulted through Firecrawl on 2026-10-20:

- [GoogleTest Rust interoperability](https://github.com/google/googletest-rust#interoperability)
  documents use with `rstest`; use fatal `assert_that!` where no non-fatal
  runner integration is needed. Do not rely on an extracted summary's
  attribute-order advice instead of the actual upstream example.
- [WHATWG URL standard](https://url.spec.whatwg.org/), especially host parsing
  and URL serialization, motivates validation after parsing numeric hosts and a
  single canonicalizer. Parsing is not a network authorization decision.
- [Verus guide](https://verus-lang.github.io/verus/guide/) defines the
  spec/proof/exec separation and scope of executable correctness claims.
- [rust-prover-tools](https://github.com/leynos/rust-prover-tools) supplies
  the pinned install/run workflow; choose and record actual release checksums
  during M1 rather than copying an unverified example pin.
