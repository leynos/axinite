//! Verus tooling smoke model: proves set-intersection properties.
//!
//! This file checks that the pinned Verus and Z3 toolchain verifies
//! well-formed proofs. It does not verify Axinite runtime behaviour. The
//! `verus` Make target and the `verus-proofs` formal-verification workflow job
//! verify it with that pinned toolchain.

use vstd::prelude::*;

verus! {

proof fn intersection_membership_iff_both(
    left: Set<int>,
    right: Set<int>,
    value: int,
)
    ensures
        left.intersect(right).contains(value)
            == (left.contains(value) && right.contains(value)),
{
    vstd::set::lemma_set_intersect(left, right, value);
}

proof fn intersection_is_subset_of_both_inputs(left: Set<int>, right: Set<int>)
    ensures
        left.intersect(right).subset_of(left),
        left.intersect(right).subset_of(right),
{
    assert forall|value: int|
        #![trigger left.intersect(right).contains(value)]
        !left.intersect(right).contains(value) || left.contains(value)
    by {
        intersection_membership_iff_both(left, right, value);
    };
    assert forall|value: int|
        #![trigger left.intersect(right).contains(value)]
        !left.intersect(right).contains(value) || right.contains(value)
    by {
        intersection_membership_iff_both(left, right, value);
    };
}

proof fn empty_and_nonempty_intersection_witnesses()
    ensures
        !Set::<int>::empty().intersect(Set::<int>::empty()).contains(0),
        Set::<int>::empty().insert(1)
            .intersect(Set::<int>::empty().insert(1))
            .contains(1),
{
    let empty = Set::<int>::empty();
    let singleton = empty.insert(1);

    intersection_membership_iff_both(empty, empty, 0);
    vstd::set::lemma_set_empty(0int);

    intersection_membership_iff_both(singleton, singleton, 1);
    vstd::set::lemma_set_insert_same(empty, 1);
}

fn main() {}

}
