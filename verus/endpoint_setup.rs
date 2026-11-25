//! Verus model of the typed setup planner and endpoint binding kernel.
//!
//! This is a *model* proof: it re-expresses the pure planning, scoped
//! application, and readiness projection rules from
//! `src/extensions/setup/policy.rs` over Verus sequences and maps. Payloads
//! (credential values and canonical endpoint URLs) are opaque `nat` atoms, so
//! no proof here reasons about URL syntax or cryptography. The refinement
//! mapping between this model and production code is recorded in
//! `verus/README.md`.
//!
//! Proved obligations:
//!
//! - **P1 partition and conservation.** An accepted plan has exactly one
//!   effect per submitted operation, in order; rejected inputs produce no
//!   plan; no effect is invented.
//! - **P2 scope preservation.** Applying a sequence of endpoint effects
//!   changes only the keys named by those effects, and clearing is
//!   idempotent.
//! - **P3 projection and readiness.** The public summary ignores payload
//!   atoms, and readiness holds exactly when authentication holds and every
//!   declared field is either available or optionally missing.
//!
//! Run with `make verus VERUS_PROOF_FILE=verus/endpoint_setup.rs`.

use vstd::prelude::*;
use vstd::seq::{lemma_seq_new_index, lemma_seq_new_len};

verus! {

/// Whether a declaration addresses the credential store or the endpoint
/// store.
pub enum Kind {
    Secret,
    Endpoint,
}

/// The operation variants accepted by the planner.
pub enum OpKind {
    SecretSet,
    EndpointSet,
    EndpointClear,
}

/// A manifest declaration with a unique field key.
pub struct Decl {
    pub key: nat,
    pub kind: Kind,
    pub required: bool,
}

/// A submitted operation. `payload` is an opaque value atom.
pub struct Op {
    pub key: nat,
    pub kind: OpKind,
    pub payload: nat,
}

/// A planned effect. Payload atoms are carried through unchanged.
pub struct Effect {
    pub key: nat,
    pub kind: OpKind,
    pub payload: nat,
}

pub open spec fn effect_of(op: Op) -> Effect {
    Effect { key: op.key, kind: op.kind, payload: op.payload }
}

/// The plan for a sequence of operations, index for index.
pub open spec fn plan(ops: Seq<Op>) -> Seq<Effect> {
    Seq::new(ops.len(), |i: int| effect_of(ops[i]))
}

/// Look up the declared kind for a key, if any.
pub open spec fn declared_kind(decls: Seq<Decl>, key: nat) -> Option<Kind>
    decreases decls.len(),
{
    if decls.len() == 0 {
        None
    } else if decls[0].key == key {
        Some(decls[0].kind)
    } else {
        declared_kind(decls.subrange(1, decls.len() as int), key)
    }
}

/// Whether an operation targets a declaration of the matching kind.
pub open spec fn op_matches(decls: Seq<Decl>, op: Op) -> bool {
    match op.kind {
        OpKind::SecretSet => declared_kind(decls, op.key) == Some(Kind::Secret),
        OpKind::EndpointSet => declared_kind(decls, op.key) == Some(Kind::Endpoint),
        OpKind::EndpointClear => declared_kind(decls, op.key) == Some(Kind::Endpoint),
    }
}

/// Submitted keys are unique, so no field is planned twice.
pub open spec fn unique_keys(ops: Seq<Op>) -> bool {
    forall|i: int, j: int| 0 <= i < j < ops.len() ==> ops[i].key != ops[j].key
}

/// The planner's acceptance predicate: unique keys and every operation
/// matching a declaration of its own kind.
pub open spec fn accepted(decls: Seq<Decl>, ops: Seq<Op>) -> bool {
    unique_keys(ops) && forall|i: int| 0 <= i < ops.len() ==> op_matches(decls, ops[i])
}

/// A checked plan: rejection yields no operations at all.
pub open spec fn plan_checked(decls: Seq<Decl>, ops: Seq<Op>) -> Option<Seq<Effect>> {
    if accepted(decls, ops) {
        Some(plan(ops))
    } else {
        None
    }
}

// --- P1: partition and conservation --------------------------------------

pub proof fn p1_length(decls: Seq<Decl>, ops: Seq<Op>)
    requires
        accepted(decls, ops),
    ensures
        plan(ops).len() == ops.len(),
{
    lemma_seq_new_len(ops.len(), |i: int| effect_of(ops[i]));
}

pub proof fn p1_correspondence(decls: Seq<Decl>, ops: Seq<Op>, i: int)
    requires
        accepted(decls, ops),
        0 <= i < ops.len(),
    ensures
        plan(ops)[i] == effect_of(ops[i]),
        plan(ops)[i].key == ops[i].key,
        plan(ops)[i].kind == ops[i].kind,
        plan(ops)[i].payload == ops[i].payload,
{
    lemma_seq_new_index(ops.len(), |i: int| effect_of(ops[i]), i);
}

pub proof fn p1_rejection_produces_no_operations(decls: Seq<Decl>, ops: Seq<Op>)
    requires
        !accepted(decls, ops),
    ensures
        plan_checked(decls, ops) is None,
{
}

pub proof fn p1_accepted_produces_one_effect_per_operation(decls: Seq<Decl>, ops: Seq<Op>)
    requires
        accepted(decls, ops),
    ensures
        plan_checked(decls, ops) == Some(plan(ops)),
        plan_checked(decls, ops) matches Some(effects) && effects.len() == ops.len(),
{
    p1_length(decls, ops);
}

/// No effect names a key that no operation named.
pub proof fn p1_no_invented_operations(ops: Seq<Op>, key: nat)
    requires
        forall|i: int| 0 <= i < ops.len() ==> ops[i].key != key,
    ensures
        forall|j: int| 0 <= j < plan(ops).len() ==> plan(ops)[j].key != key,
{
    assert forall|j: int| 0 <= j < plan(ops).len() implies plan(ops)[j].key != key by {
        lemma_seq_new_index(ops.len(), |i: int| effect_of(ops[i]), j);
    };
}

/// Under unique submitted keys, an effect occurs at most once.
pub proof fn p1_effect_occurs_at_most_once(ops: Seq<Op>, i: int, j: int)
    requires
        unique_keys(ops),
        0 <= i < ops.len(),
        0 <= j < ops.len(),
        effect_of(ops[i]) == effect_of(ops[j]),
    ensures
        i == j,
{
    if i < j {
        assert(ops[i].key == ops[j].key);
    }
}

// --- P2: scoped application ----------------------------------------------

/// Keys named by an effect sequence.
pub open spec fn targets(effects: Seq<Effect>) -> Set<nat>
    decreases effects.len(),
{
    if effects.len() == 0 {
        Set::empty()
    } else {
        targets(effects.subrange(0, effects.len() as int - 1)).insert(
            effects[effects.len() as int - 1].key,
        )
    }
}

/// Apply one effect. Secret writes do not touch the endpoint store.
pub open spec fn apply_effect(store: Map<nat, nat>, effect: Effect) -> Map<nat, nat> {
    match effect.kind {
        OpKind::SecretSet => store,
        OpKind::EndpointSet => store.insert(effect.key, effect.payload),
        OpKind::EndpointClear => store.remove(effect.key),
    }
}

/// Apply endpoint effects to a scoped endpoint store.
pub open spec fn apply(store: Map<nat, nat>, effects: Seq<Effect>) -> Map<nat, nat>
    decreases effects.len(),
{
    if effects.len() == 0 {
        store
    } else {
        let prefix = effects.subrange(0, effects.len() as int - 1);
        let effect = effects[effects.len() as int - 1];
        apply_effect(apply(store, prefix), effect)
    }
}

/// A one-element effect sequence is exactly one application.
pub proof fn apply_singleton(store: Map<nat, nat>, effect: Effect)
    ensures
        apply(store, seq![effect]) == apply_effect(store, effect),
{
    let effects = seq![effect];
    assert(effects.len() == 1);
    assert(effects[effects.len() as int - 1] == effect);
    assert(effects.subrange(0, 0) =~= Seq::<Effect>::empty());
    assert(apply(store, effects.subrange(0, 0)) == store);
}

pub proof fn p2_preserves_other_keys(store: Map<nat, nat>, effects: Seq<Effect>, key: nat)
    requires
        !targets(effects).contains(key),
    ensures
        apply(store, effects).dom().contains(key) == store.dom().contains(key),
        store.dom().contains(key) ==> apply(store, effects)[key] == store[key],
    decreases effects.len(),
{
    if effects.len() == 0 {
    } else {
        let prefix = effects.subrange(0, effects.len() as int - 1);
        let effect = effects[effects.len() as int - 1];
        assert(!targets(prefix).contains(key));
        assert(key != effect.key);
        p2_preserves_other_keys(store, prefix, key);
        match effect.kind {
            OpKind::SecretSet => {},
            OpKind::EndpointSet => {},
            OpKind::EndpointClear => {},
        }
    }
}

/// Replacing one key preserves every other stored value.
pub proof fn p2_replace_preserves_others(store: Map<nat, nat>, effect: Effect, other: nat)
    requires
        effect.kind == OpKind::EndpointSet,
        other != effect.key,
    ensures
        apply(store, seq![effect]).dom().contains(other) == store.dom().contains(other),
        store.dom().contains(other) ==> apply(store, seq![effect])[other] == store[other],
{
    let effects = seq![effect];
    apply_singleton(store, effect);
    assert(apply(store, effects) == apply_effect(store, effect));
    assert(apply_effect(store, effect) == store.insert(effect.key, effect.payload));
    let applied = apply(store, effects);
    assert(applied == store.insert(effect.key, effect.payload));
    assert(applied.dom() == store.dom().insert(effect.key));
    assert(applied.dom().contains(other) == store.dom().contains(other));
    if store.dom().contains(other) {
        assert(applied[other] == store[other]);
    }
}

pub open spec fn clear_effect(key: nat) -> Effect {
    Effect { key, kind: OpKind::EndpointClear, payload: 0 }
}

/// Clearing an absent or present key twice equals clearing it once.
pub proof fn p2_clear_is_idempotent(store: Map<nat, nat>, key: nat)
    ensures
        apply(apply(store, seq![clear_effect(key)]), seq![clear_effect(key)]) == apply(
            store,
            seq![clear_effect(key)],
        ),
{
    let effects = seq![clear_effect(key)];
    apply_singleton(store, clear_effect(key));
    assert(apply(store, effects) == apply_effect(store, clear_effect(key)));
    assert(apply_effect(store, clear_effect(key)) == store.remove(key));
    let once = apply(store, effects);
    assert(once == store.remove(key));
    apply_singleton(once, clear_effect(key));
    let twice = apply(once, effects);
    assert(twice == apply_effect(once, clear_effect(key)));
    assert(apply_effect(once, clear_effect(key)) == once.remove(key));
    assert(twice == once.remove(key));
    assert(twice.dom() =~= once.dom());
    assert forall|other: nat| #[trigger] once.dom().contains(other) implies twice[other] == once[other] by {
        if other != key {
            assert(once.remove(key)[other] == once[other]);
        } else {
            assert(!twice.dom().contains(key));
        }
    };
    assert(twice =~= once);
}

// --- P3: projection and readiness ----------------------------------------

/// Availability of one declared field. `Invalid` covers kind or policy
/// mismatch; `Error` covers a failed read or authentication.
pub enum Avail {
    Missing,
    Present,
    Invalid,
    Error,
}

pub struct FieldState {
    pub key: nat,
    pub kind: Kind,
    pub required: bool,
    pub avail: Avail,
}

/// A field permits readiness when it is available, or optional and merely
/// missing. Invalid and errored fields always block, optional or not.
pub open spec fn field_status_ok(state: FieldState) -> bool {
    match state.avail {
        Avail::Present => true,
        Avail::Missing => !state.required,
        Avail::Invalid => false,
        Avail::Error => false,
    }
}

pub open spec fn all_fields_ok(states: Seq<FieldState>) -> bool
    decreases states.len(),
{
    if states.len() == 0 {
        true
    } else {
        field_status_ok(states[0]) && all_fields_ok(
            states.subrange(1, states.len() as int),
        )
    }
}

/// Readiness: authentication holds and every declared field is acceptable.
pub open spec fn ready(states: Seq<FieldState>, auth_ok: bool) -> bool {
    auth_ok && all_fields_ok(states)
}

pub open spec fn field_statuses(states: Seq<FieldState>) -> Seq<Avail> {
    Seq::new(states.len(), |i: int| states[i].avail)
}

pub struct Summary {
    pub ready: bool,
    pub statuses: Seq<Avail>,
}

/// The public summary. The `payloads` argument is accepted only to make the
/// independence obligation explicit; it is not part of the result.
pub open spec fn public_summary(
    states: Seq<FieldState>,
    auth_ok: bool,
    payloads: Map<nat, nat>,
) -> Summary {
    Summary { ready: ready(states, auth_ok), statuses: field_statuses(states) }
}

/// Equal declarations, availability, and auth state yield equal public
/// summaries regardless of confidential payload contents.
pub proof fn p3_public_summary_ignores_payloads(
    states: Seq<FieldState>,
    auth_ok: bool,
    payloads_a: Map<nat, nat>,
    payloads_b: Map<nat, nat>,
)
    ensures
        public_summary(states, auth_ok, payloads_a) == public_summary(
            states,
            auth_ok,
            payloads_b,
        ),
{
}

/// A single acceptable field makes a singleton state list acceptable.
pub proof fn singleton_fields_ok(state: FieldState)
    requires
        field_status_ok(state),
    ensures
        all_fields_ok(seq![state]),
{
    let states = seq![state];
    assert(states.len() == 1);
    assert(states[0] == state);
    assert(states.subrange(1, 1) =~= Seq::<FieldState>::empty());
    assert(all_fields_ok(Seq::<FieldState>::empty()));
}

/// A single unacceptable field makes a singleton state list unacceptable,
/// even when every other rule would pass.
pub proof fn singleton_field_not_ok(state: FieldState)
    requires
        !field_status_ok(state),
    ensures
        !all_fields_ok(seq![state]),
{
    let states = seq![state];
    assert(states.len() == 1);
    assert(states[0] == state);
    if all_fields_ok(states) {
        assert(field_status_ok(states[0]));
    }
}

pub proof fn p3_required_present_with_auth_is_ready()
    ensures
        ready(
            seq![FieldState {
                key: 0,
                kind: Kind::Endpoint,
                required: true,
                avail: Avail::Present,
            }],
            true,
        ),
{
    singleton_fields_ok(
        FieldState {
            key: 0,
            kind: Kind::Endpoint,
            required: true,
            avail: Avail::Present,
        },
    );
}

pub proof fn p3_required_missing_is_not_ready()
    ensures
        !ready(
            seq![FieldState {
                key: 0,
                kind: Kind::Endpoint,
                required: true,
                avail: Avail::Missing,
            }],
            true,
        ),
{
    singleton_field_not_ok(
        FieldState {
            key: 0,
            kind: Kind::Endpoint,
            required: true,
            avail: Avail::Missing,
        },
    );
}

pub proof fn p3_optional_missing_with_auth_is_ready()
    ensures
        ready(
            seq![FieldState {
                key: 0,
                kind: Kind::Secret,
                required: false,
                avail: Avail::Missing,
            }],
            true,
        ),
{
    singleton_fields_ok(
        FieldState {
            key: 0,
            kind: Kind::Secret,
            required: false,
            avail: Avail::Missing,
        },
    );
}

pub proof fn p3_optional_error_blocks_readiness()
    ensures
        !ready(
            seq![FieldState {
                key: 0,
                kind: Kind::Secret,
                required: false,
                avail: Avail::Error,
            }],
            true,
        ),
{
    singleton_field_not_ok(
        FieldState {
            key: 0,
            kind: Kind::Secret,
            required: false,
            avail: Avail::Error,
        },
    );
}

pub proof fn p3_invalid_blocks_readiness()
    ensures
        !ready(
            seq![FieldState {
                key: 0,
                kind: Kind::Endpoint,
                required: false,
                avail: Avail::Invalid,
            }],
            true,
        ),
{
    singleton_field_not_ok(
        FieldState {
            key: 0,
            kind: Kind::Endpoint,
            required: false,
            avail: Avail::Invalid,
        },
    );
}

pub proof fn p3_auth_failure_blocks_readiness()
    ensures
        !ready(
            seq![FieldState {
                key: 0,
                kind: Kind::Endpoint,
                required: true,
                avail: Avail::Present,
            }],
            false,
        ),
{
}

/// A two-field state list is unacceptable when its second field is.
pub proof fn pair_fields_ok(first: FieldState, second: FieldState)
    requires
        field_status_ok(first),
        field_status_ok(second),
    ensures
        all_fields_ok(seq![first, second]),
{
    let states = seq![first, second];
    assert(states.len() == 2);
    assert(states[0] == first);
    assert(states.subrange(1, 2) =~= seq![second]);
    singleton_fields_ok(second);
}

/// A two-field state list is unacceptable when either field is.
pub proof fn pair_fields_not_ok(first: FieldState, second: FieldState)
    requires
        !field_status_ok(second),
    ensures
        !all_fields_ok(seq![first, second]),
{
    let states = seq![first, second];
    assert(states.len() == 2);
    assert(states[0] == first);
    assert(states.subrange(1, 2) =~= seq![second]);
    singleton_field_not_ok(second);
}

/// A valid OAuth token cannot bypass a missing required endpoint.
pub proof fn p3_valid_token_cannot_bypass_missing_endpoint()
    ensures
        !ready(
            seq![
                FieldState { key: 0, kind: Kind::Secret, required: true, avail: Avail::Present },
                FieldState {
                    key: 1,
                    kind: Kind::Endpoint,
                    required: true,
                    avail: Avail::Missing,
                },
            ],
            true,
        ),
{
    pair_fields_not_ok(
        FieldState { key: 0, kind: Kind::Secret, required: true, avail: Avail::Present },
        FieldState {
            key: 1,
            kind: Kind::Endpoint,
            required: true,
            avail: Avail::Missing,
        },
    );
}

/// Endpoint availability does not substitute for authentication.
pub proof fn p3_endpoint_presence_does_not_satisfy_auth()
    ensures
        !ready(
            seq![FieldState {
                key: 0,
                kind: Kind::Endpoint,
                required: true,
                avail: Avail::Present,
            }],
            false,
        ),
{
}

fn main() {}

}