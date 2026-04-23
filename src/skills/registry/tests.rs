//! Test suite for [`SkillRegistry`].
//!
//! Tests are split into focused sub-modules by subsystem:
//! - [`discovery`]: skill discovery across directory layouts, gating, and
//!   platform edge-cases.
//! - [`install`]: staged install, bundle materialisation, and commit/cleanup
//!   lifecycle.
//! - [`lookup`]: `has`, `find_by_name`, hash computation, and trust semantics.
//! - [`removal`]: user-skill removal, flat-layout targeting, and rejection of
//!   non-user sources.
//!
//! Shared fixtures and write helpers live in [`fixtures`].
mod discovery;
mod fixtures;
mod install;
mod lookup;
mod removal;
