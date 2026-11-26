#![forbid(unsafe_code)]

//! Isolated Monty interpreter helper for Axinite `exec_code`.
//!
//! The helper speaks only the framed protocol in `monty-protocol`. It receives
//! one run, yields tool callbacks to the parent, and reports one terminal
//! outcome. It performs no filesystem, network or process side effects of its
//! own, and imposes an address-space ceiling on itself before reading any
//! guest source.

#![deny(missing_docs)]

use monty_alloc::LimitedAllocator;

mod convert;
mod limits;
mod session;
mod source;

/// Counts every allocation and ends the process if the armed budget is
/// exceeded.
#[global_allocator]
static ALLOCATOR: LimitedAllocator = LimitedAllocator;

fn main() {
    let code = session::serve_stdio();
    if code != 0 {
        std::process::exit(code);
    }
}
