# Architectural decision record (ADR) 014: Confidential endpoint setup

## Status

Proposed. Implementation requires explicit approval of the
[2.1.1 ExecPlan](execplans/2-1-1-typed-setup-fields-and-endpoint-binding-persistence.md).

## Date

2026-10-20.

## Context and problem statement

[RFC 0004](rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md)
requires extension setup to distinguish credentials from confidential endpoint
configuration. Existing setup stores every value as a secret; the existing
cryptographic API authenticates ciphertext without binding it to its database
record identity. Both PostgreSQL and libSQL must support the new persistence
contract, and both the Solid and legacy extension interfaces remain supported.

The scope is roadmap 2.1.1. It provides configuration and persistence, not the
later delegated request capability or network authorization path.

## Decision drivers

- Endpoint URLs must stay out of guest/model surfaces, responses, and logs.
- A user must be able to replace or clear a binding without reading it back.
- Existing installed secret-only extension manifests must remain usable.
- Storage failures must not masquerade as missing fields or successful setup.
- Proof obligations must remain connected to executable policy.

## Options considered

Storing endpoints in `SecretsStore` would reuse existing machinery but assign
credential semantics to configuration and invite incorrect exposure paths.
Plaintext generic settings would separate semantics but fail confidentiality at
rest. A dedicated encrypted binding store preserves both requirements. A new
general-purpose vault or distributed transaction coordinator would add
unrelated infrastructure and is not warranted for this phase.

## Decision outcome / proposed direction

Use a dedicated endpoint service and narrow repository port. Bind records to
`(user_id, extension_name, endpoint_name)` and support multiple named endpoints
per extension. The authenticated host supplies the first two identifiers;
manifest declarations supply endpoint names and kinds. Each field key equals
its destination name; normalized declarations cannot alias one stored value. No
cross-extension sharing is introduced.

Reuse the existing authenticated encryption mechanics with endpoint-specific
domain separation and associated data covering scope, kind, and format version
and validation-policy version. Associated data is authenticated metadata,
encoded unambiguously so different scope tuples cannot collide. Preserve
existing secret ciphertext compatibility. Do not persist plaintext origin/path
projections or free-form errors. Public summaries contain identifiers and
status only.

Validation parses and canonicalizes an absolute HTTPS URL, rejects credentials,
queries, fragments, local-only hostnames, and non-public IP literals, and never
contacts the target. Configured means structurally validated and stored.
Connection-time DNS, private-address checks, redirect policy, grants, and
approvals remain mandatory future transport work, not implied by setup status.

Validate complete submissions before effects. Each binding update is atomic;
mixed credential/endpoint persistence can partially commit on I/O failure.
Return safe per-field outcomes (committed, unchanged, indeterminate, or
unattempted), reload status, and avoid success/activation on that failure. Lost
commit acknowledgements are indeterminate; status-only reload cannot identify
which concurrent replacement won. Retry only on explicit user action,
acknowledging that it may overwrite an intervening save. Do not compensate by
overwriting potentially concurrent updates. Endpoint writes use
last-commit-wins semantics. Omission keeps a value, explicit clear deletes
idempotently, and a rejected replacement preserves the old value.

Readiness combines required setup availability with existing authentication
requirements, preserving approved auto-resolved intermediate OAuth fields. Any
declared field in an error/invalid state blocks readiness, even if optional.
Status reads authenticate records and check current kind/policy, not just row
existence. Stale records require explicit replace/clear; undeclared rows remain
hidden and retained for rollback until extension removal cleans that scope.
Endpoint-only edits neither delete OAuth tokens nor initiate reauthentication.
Corrupt or inaccessible storage is an error, not absence.

Keep the pure operation planner and public projection separate from I/O.
Specify routing conservation, scoped update preservation, and
payload-independent status projections before implementing them. Use Verus for
these selected unbounded properties and maintain an explicit production/model
relationship. The proof does not cover crypto, SQL, parsers, or arbitrary
information flow; backend, integration, property, and browser tests cover those
boundaries.

## Migration plan

Normalize legacy `required_secrets` declarations into the typed field model at
the manifest boundary. Reject ambiguous mixed declarations. Update internal
setup DTOs and both bundled frontends together rather than introducing source
compatibility wrappers. Add fresh, additive migrations for both databases; do
not rewrite existing migrations. Preserve the binding table on application
rollback and keep encryption keys available for roll-forward.

## Known risks and limitations

Offline validation does not establish endpoint reachability or safe DNS
resolution. Encryption does not protect a compromised host or erase previous
copies in backups/WAL files. Concurrent administrators can overwrite each
other's changes under last-commit-wins semantics. Mixed saves may partially
commit, so the UI must explain recovery without echoing confidential input.

A separate Verus model can drift from production. The ExecPlan requires
refinement evidence, negative controls, and an explicit statement of any
remaining bridge gap. Unsupported proof representations require a reviewed
deviation rather than an assumed theorem.

## Outstanding decisions

Approval must accept the proposed scope, offline validation meaning, rejection
of endpoint query strings, partial-save contract, and proof boundary. Concrete
module splits, migration numbers, pinned proof release/checksum, and test
fixture identifiers are implementation details to record as work proceeds.
