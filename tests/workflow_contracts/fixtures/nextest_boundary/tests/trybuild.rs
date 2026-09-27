//! Stands in for the real `trybuild` compile-contract binary.
//!
//! Only the target name matters: the real overrides name
//! `binary(trybuild)`, and nextest refuses a filterset naming a binary
//! the package does not declare.

#[test]
fn the_binary_exists() {}
