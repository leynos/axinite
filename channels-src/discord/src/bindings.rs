//! Generated WebAssembly component bindings for the `sandboxed-channel` world.
//!
//! This module is the crate's only unsafe boundary. `wit_bindgen::generate!`
//! and `export!` expand to the canonical-ABI shims that the host calls by
//! symbol name: `unsafe fn` lifting and lowering glue and `export_name`
//! entry points. No safe formulation of those exists, so `lib.rs` declares
//! this module with `#[expect(unsafe_code, reason = "wit-bindgen component
//! export ABI")]` while the rest of the crate denies unsafe code.
//!
//! Keep this file free of handwritten logic. The guest implementation lives
//! in the safe modules and reaches the bindings only through the `exports` and
//! `near` re-exports in `lib.rs`. The boundary contract
//! (`scripts/check_unsafe_boundaries.py`) rejects any other content.

use super::DiscordChannel;

wit_bindgen::generate!({
    world: "sandboxed-channel",
    path: "../../wit/channel.wit",
});

export!(DiscordChannel);

// The generated return area is laid out for 4-byte pointers, so these tests
// only build for a 32-bit target; `make miri` runs them for i686.
#[cfg(all(test, target_pointer_width = "32"))]
mod abi_round_trip;
