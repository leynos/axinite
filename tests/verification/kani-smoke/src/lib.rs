//! A small bounded capability-projection property for Kani tooling checks.

const FOUR_ENTRY_MASK: u8 = 0b0000_1111;

/// Retain requested capability entries that are also permitted.
pub fn exact_projection(requested: u8, permitted: u8) -> u8 {
    requested & permitted & FOUR_ENTRY_MASK
}

/// Kani harnesses that check `exact_projection` against the four-entry mask.
///
/// The harness explores arbitrary `requested` and `permitted` bytes and
/// asserts that each of the four projected entries matches the corresponding
/// requested-and-permitted membership, and that no higher bit survives.
#[cfg(kani)]
mod proofs {
    use super::{exact_projection, FOUR_ENTRY_MASK};

    #[kani::proof]
    #[kani::unwind(5)]
    fn verify_exact_projection_four_entries() {
        let requested: u8 = kani::any();
        let permitted: u8 = kani::any();
        let projection = exact_projection(requested, permitted);

        for bit_index in 0..4 {
            let expected_membership = match bit_index {
                0 => requested & 0b0000_0001 != 0 && permitted & 0b0000_0001 != 0,
                1 => requested & 0b0000_0010 != 0 && permitted & 0b0000_0010 != 0,
                2 => requested & 0b0000_0100 != 0 && permitted & 0b0000_0100 != 0,
                3 => requested & 0b0000_1000 != 0 && permitted & 0b0000_1000 != 0,
                _ => unreachable!("the bounded loop visits four entries"),
            };
            let projected_membership = match bit_index {
                0 => projection & 0b0000_0001 != 0,
                1 => projection & 0b0000_0010 != 0,
                2 => projection & 0b0000_0100 != 0,
                3 => projection & 0b0000_1000 != 0,
                _ => unreachable!("the bounded loop visits four entries"),
            };

            assert!(
                projected_membership == expected_membership,
                "projected membership must match each requested and permitted entry"
            );
        }

        assert_eq!(projection & !FOUR_ENTRY_MASK, 0);
        assert_eq!(exact_projection(0, 0), 0);
        assert_eq!(exact_projection(0b0000_0101, 0b0000_1010), 0);
        assert_eq!(exact_projection(0b1111_1111, 0b1010_0101), 0b0000_0101);
    }
}
