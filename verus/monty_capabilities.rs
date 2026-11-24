//! Verus proof of the shipped Monty capability-projection kernel.
//!
//! The executable bodies between `KERNEL-BODY` markers are byte-identical to
//! `crates/monty-capabilities/src/lib.rs`; the drift test enforces that. This
//! file adds the specifications and proofs that the shipped bodies satisfy
//! them. Verus verifies these functions, not the parent's table construction,
//! registry freshness, approval, or process lifecycle.

use vstd::prelude::*;

verus! {

/// Specification for the selected capability view.
pub open spec fn project_spec(available: u64, requested: u64, validated: u64) -> u64 {
    available & requested & validated
}

/// Specification for admission of one table index.
pub open spec fn admits_spec(view: u64, index: u64) -> bool {
    index < 64 && (view >> index) & 1 == 1
}

/// Specification for "every bit of the view is allowed".
pub open spec fn subset_spec(view: u64, allowed: u64) -> bool {
    view & !allowed == 0
}

/// The shipped projection body, proved equal to `project_spec`.
pub fn project(available: u64, requested: u64, validated: u64) -> (result: u64)
    ensures
        result == project_spec(available, requested, validated),
{
    // KERNEL-BODY: project
    available & requested & validated
    // END KERNEL-BODY: project
}

/// The shipped admission body, proved equal to `admits_spec`.
pub fn admits(view: u64, index: u64) -> (result: bool)
    ensures
        result == admits_spec(view, index),
{
    // KERNEL-BODY: admits
    index < 64 && (view >> index) & 1 == 1
    // END KERNEL-BODY: admits
}

/// The shipped subset body, proved equal to `subset_spec`.
pub fn is_subset(view: u64, allowed: u64) -> (result: bool)
    ensures
        result == subset_spec(view, allowed),
{
    // KERNEL-BODY: is_subset
    view & !allowed == 0
    // END KERNEL-BODY: is_subset
}

/// The projection is exactly the intersection of its three inputs.
proof fn project_is_exact(available: u64, requested: u64, validated: u64)
    ensures
        project_spec(available, requested, validated)
            == (available & requested & validated),
{
    assert(project_spec(available, requested, validated)
        == (available & requested & validated)) by (bit_vector);
}

/// No selected bit comes from outside availability.
proof fn project_subset_available(available: u64, requested: u64, validated: u64)
    ensures
        subset_spec(project_spec(available, requested, validated), available),
{
    assert(subset_spec(project_spec(available, requested, validated), available))
        by (bit_vector);
}

/// No selected bit comes from outside the caller's request.
proof fn project_subset_requested(available: u64, requested: u64, validated: u64)
    ensures
        subset_spec(project_spec(available, requested, validated), requested),
{
    assert(subset_spec(project_spec(available, requested, validated), requested))
        by (bit_vector);
}

/// No selected bit comes from outside the validated set.
proof fn project_subset_validated(available: u64, requested: u64, validated: u64)
    ensures
        subset_spec(project_spec(available, requested, validated), validated),
{
    assert(subset_spec(project_spec(available, requested, validated), validated))
        by (bit_vector);
}

/// Narrowing availability cannot add a selected bit.
proof fn project_monotone_in_available(
    narrower: u64,
    wider: u64,
    requested: u64,
    validated: u64,
)
    requires
        // Stated bitwise so the `bit_vector` tactic can consume it; the
        // transparent [`subset_spec`] is definitionally the same condition.
        narrower & !wider == 0,
    ensures
        subset_spec(
            project_spec(narrower, requested, validated),
            project_spec(wider, requested, validated),
        ),
{
    // The projection intersects with `requested` and `validated`, so the
    // narrowed view cannot carry a bit the wider view lacks. The hypothesis is
    // repeated inside the assertion so the tactic has it as a bit-vector fact.
    assert((narrower & !wider == 0) ==> (((narrower & requested & validated) & !(wider & requested & validated)) == 0)) by (bit_vector);
}

/// Narrowing the request cannot add a selected bit.
proof fn project_monotone_in_requested(
    available: u64,
    narrower: u64,
    wider: u64,
    validated: u64,
)
    requires
        // Stated bitwise so the `bit_vector` tactic can consume it; the
        // transparent [`subset_spec`] is definitionally the same condition.
        narrower & !wider == 0,
    ensures
        subset_spec(
            project_spec(available, narrower, validated),
            project_spec(available, wider, validated),
        ),
{
    assert((narrower & !wider == 0) ==> (((available & narrower & validated) & !(available & wider & validated)) == 0)) by (bit_vector);
}

/// An index below the table size is admitted exactly when all three inputs
/// admit it.
proof fn admits_projection(
    available: u64,
    requested: u64,
    validated: u64,
    index: u64,
)
    requires
        index < 64,
    ensures
        admits_spec(project_spec(available, requested, validated), index)
            == (admits_spec(available, index)
                && admits_spec(requested, index)
                && admits_spec(validated, index)),
{
    assert(admits_spec(project_spec(available, requested, validated), index)
        == (admits_spec(available, index)
            && admits_spec(requested, index)
            && admits_spec(validated, index))) by (bit_vector);
}

/// An index outside the table is never admitted.
proof fn admits_outside_table(view: u64, index: u64)
    requires
        index >= 64,
    ensures
        !admits_spec(view, index),
{
}

fn main() {}

}