//! Trybuild compile-contract fixture for infrastructure support wiring.
//!
//! Verifies that the infrastructure support root resolves in trybuild tests and
//! exposes the expected support helpers.

#[path = "../support/infrastructure.rs"]
mod support;

fn main() {
    let _router = support::webhook_helpers::health_routes();
}
