//! Generated WebAssembly component bindings for the `sandboxed-tool` world.
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

use super::GitHubTool;

wit_bindgen::generate!({
    world: "sandboxed-tool",
    path: "../../wit/tool.wit",
});

export!(GitHubTool);
