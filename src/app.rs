//! Application builder for initializing core IronClaw components.
//!
//! Extracts the mechanical initialization phases from `main.rs` into a
//! reusable builder so that:
//!
//! - Tests can construct a full `AppComponents` without wiring channels
//! - Main stays focused on CLI dispatch and channel setup
//! - Each init phase is independently testable
//!
//! ## Two-phase bootstrap pattern
//!
//! This module follows a hexagonal architecture principle: **keep assembly
//! distinct from mechanism-heavy activation**. Construction of components
//! (the `AppBuilder`) is separated from fire-and-forget runtime side effects
//! (the `RuntimeSideEffects`).
//!
//! - Use `build_components()` when you need control over side-effect timing
//!   (e.g., in tests where I/O and background tasks should be avoided).
//! - Use `build_all()` as a convenience wrapper that constructs components,
//!   starts side effects, and waits for workspace bootstrap to finish.
//!
//! The `RuntimeSideEffects::start()` method returns task handles so callers
//! can choose whether to detach runtime work or await workspace bootstrap.
//!
//! ## Module layout
//!
//! - [`components`] — the assembled `AppComponents` bundle
//! - [`builder`] — `AppBuilder` construction and top-level assembly flow
//! - [`phases`] — the individual mechanical init phases
//! - [`side_effects`] — deferred runtime side effects and their handles

mod builder;
mod components;
mod phases;
mod side_effects;

#[cfg(test)]
mod tests;

pub use builder::{AppBuilder, AppBuilderFlags, AppBuilderParams};
pub use components::AppComponents;
pub use side_effects::{RuntimeSideEffects, RuntimeSideEffectsHandle};
