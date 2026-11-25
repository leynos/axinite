# Verus models

The proofs in this directory are **model proofs** run with the pinned Verus and
Z3 toolchain. They verify small pure kernels that mirror production decision
logic. They do not verify URL parsing, cryptography, SQL, Tokio, or the whole
running system.

Run a proof with the proof file selected explicitly:

```sh
make install-verus install-z3
make verus VERUS_PROOF_FILE=verus/endpoint_setup.rs
```

`make verus` without `VERUS_PROOF_FILE` verifies the tooling smoke model only.
The `verus-proofs` job in `.github/workflows/formal-verification.yml` runs both
the smoke model and the endpoint model.

## Refinement mapping

Each model is connected to production code by a written refinement mapping.
Changing a mapped production function requires reviewing the mapping and the
model, and updating this file when the correspondence changes. A model proof is
not evidence that unmapped Rust behaves as modelled.

### `endpoint_setup.rs` — typed setup planning

| Model item                  | Production counterpart                                         | Relationship                                                                                                                                                                                            |
| --------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Decl`                      | `extensions::setup::model::SetupField`                         | Key is the field key (`CredentialName` or `EndpointName`); kind is `Secret`/`Endpoint`; `required` is `!is_optional()`.                                                                                 |
| `Op`                        | `extensions::setup::model::SetupOperation`                     | `SecretSet` includes blank submissions; blank secret values are resolved to `KeepSecret` after planning. Payload atoms stand for submitted values.                                                      |
| `Effect`                    | `extensions::setup::policy::PlannedEffect`                     | One effect per submitted operation, in submission order. `KeepSecret` is modelled as an `OpKind::SecretSet` that does not touch the endpoint store.                                                     |
| `accepted` / `plan_checked` | `extensions::setup::policy::plan_setup`                        | Acceptance covers unique keys, declared keys, and matching kinds. Production additionally returns typed validation errors before producing effects, matching `plan_checked = None`.                     |
| `plan`                      | `plan_setup` effect vector                                     | Index `i` of the plan is the resolution of operation `i`; production appends in the same order.                                                                                                         |
| `apply`                     | `EndpointBindingService` per-effect application                | Only endpoint effects change the scoped endpoint store. Secret writes go to a separate store, modelled as untouched by `apply`.                                                                         |
| `targets`                   | Scoped binding keys `(user_id, extension_name, endpoint_name)` | The model abstracts the host scope into the key `nat`; the service fixes scope outside the payload and never accepts it from submitted values.                                                          |
| `FieldState` / `Avail`      | `EndpointBindingService` status plus credential availability   | `Missing` = no row recorded; `Present` = authenticated record whose kind and policy version match the declaration; `Invalid` = stale policy or kind mismatch; `Error` = read or authentication failure. |
| `ready`                     | `ExtensionManager` aggregate readiness                         | Authentication requirements are folded in as `auth_ok`; every declared field must satisfy `field_status_ok`, including optional fields with errors.                                                     |
| `public_summary`            | Web/API setup response projection                              | Identifiers, kind, optionality, and status only; payload atoms never appear.                                                                                                                            |

The proof deliberately abstracts the two stores into one endpoint map plus an
opaque credential payload, so P2 scope preservation speaks about the endpoint
store only. Credential writes are covered by P1 partition conservation (they
appear exactly once in the plan) and by the service contract tests.

## Obligations

`endpoint_setup.rs` proves:

- **P1** — accepted plans have exactly one effect per operation in order
  (`p1_length`, `p1_correspondence`), rejection produces no plan
  (`p1_rejection_produces_no_operations`), no effect is invented
  (`p1_no_invented_operations`), and a unique-key submission produces at most
  one effect per key (`p1_effect_occurs_at_most_once`).
- **P2** — applying effects leaves every untargeted key unchanged
  (`p2_preserves_other_keys`, `p2_replace_preserves_others`), and clearing is
  idempotent (`p2_clear_is_idempotent`).
- **P3** — the public summary is independent of payload atoms
  (`p3_public_summary_ignores_payloads`), and readiness matches the
  required/optional/auth rules with witnesses for each case
  (`p3_required_present_with_auth_is_ready`, `p3_required_missing_is_not_ready`,
  `p3_optional_missing_with_auth_is_ready`,
  `p3_optional_error_blocks_readiness`, `p3_invalid_blocks_readiness`,
  `p3_auth_failure_blocks_readiness`,
  `p3_valid_token_cannot_bypass_missing_endpoint`,
  `p3_endpoint_presence_does_not_satisfy_auth`).

The proof uses no `assume`, `admit`, or external body. It does not reason about
URL syntax, ciphertext, or database atomicity; those are covered by unit,
contract, and integration tests.
