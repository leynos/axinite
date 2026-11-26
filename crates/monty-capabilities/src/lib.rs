#![forbid(unsafe_code)]

//! The capability-projection kernel shared by production and Verus.
//!
//! A run's selected-tool table is an immutable, name-sorted slice of at most
//! [`MAX_SELECTED_TOOLS`] entries. Availability (`A`), the caller's request
//! (`R`) and schema validation (`V`) are bitmasks over that table. The selected
//! capability view is exactly `A ∩ R ∩ V`.
//!
//! The executable bodies between `KERNEL-BODY` markers are repeated verbatim
//! in `verus/monty_capabilities.rs`, where they are proved against
//! specifications. `tests/drift.rs` compares the marked bodies byte for byte,
//! so the proved body is the shipped body. Do not edit a marked body without
//! updating the Verus file.

#![deny(missing_docs)]

/// Maximum number of tools addressable by a `u64` capability mask.
pub const MAX_SELECTED_TOOLS: usize = 64;

/// Projects the caller's selected view as the intersection of availability,
/// request and validation masks.
///
/// A bit is present in the result exactly when it is present in all three
/// inputs. No other bit can be set.
pub fn project(available: u64, requested: u64, validated: u64) -> u64 {
    // KERNEL-BODY: project
    available & requested & validated
    // END KERNEL-BODY: project
}

/// Whether `index` is admitted by `view`.
///
/// Indices at or beyond [`MAX_SELECTED_TOOLS`] are never admitted.
pub fn admits(view: u64, index: u64) -> bool {
    // KERNEL-BODY: admits
    index < 64 && (view >> index) & 1 == 1
    // END KERNEL-BODY: admits
}

/// Whether every bit of `view` is also present in `allowed`.
pub fn is_subset(view: u64, allowed: u64) -> bool {
    // KERNEL-BODY: is_subset
    view & !allowed == 0
    // END KERNEL-BODY: is_subset
}

/// Builds a mask from zero-based table indices, ignoring entries beyond the
/// 64-entry table.
///
/// This helper constructs inputs for [`project`]; it is not part of the proved
/// kernel.
pub fn mask_from_indices(indices: impl IntoIterator<Item = usize>) -> u64 {
    indices.into_iter().fold(0_u64, |mask, index| {
        if index < MAX_SELECTED_TOOLS {
            mask | (1_u64 << index)
        } else {
            mask
        }
    })
}

#[cfg(test)]
mod tests {
    //! Unit tests for the capability mask kernel.

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn projection_is_exact_intersection() {
        let available = 0b1011_u64;
        let requested = 0b1110_u64;
        let validated = 0b0111_u64;
        assert_eq!(project(available, requested, validated), 0b0010);
    }

    #[test]
    fn projection_cannot_widen_any_input() {
        let available = 0b0101_u64;
        let requested = 0b1010_u64;
        let validated = u64::MAX;
        let view = project(available, requested, validated);
        assert!(is_subset(view, available));
        assert!(is_subset(view, requested));
        assert!(is_subset(view, validated));
    }

    #[test]
    fn omitted_request_clears_the_bit() {
        let full = u64::MAX;
        let without_tool_3 = full & !(1_u64 << 3);
        assert!(admits(project(full, full, full), 3));
        assert!(!admits(project(full, without_tool_3, full), 3));
    }

    #[test]
    fn index_beyond_table_is_never_admitted() {
        assert!(!admits(u64::MAX, 64));
        assert!(!admits(u64::MAX, u64::MAX));
    }

    #[test]
    fn mask_from_indices_ignores_out_of_table_entries() {
        assert_eq!(mask_from_indices([0, 2, 64, 200]), 0b101);
    }
}
