//! Installation smoke proof: compiles only this code under test.

#[kani::proof]
fn binary_installation_smoke() {
    let value: u8 = kani::any();
    assert_eq!(u16::from(value) + 1 - 1, u16::from(value));
}
