//! Focused tests for the rig adapter's request, conversion, and helper logic.

pub(super) use super::request::build_rig_request;
pub(super) use super::*;
pub(super) use crate::llm::test_fixtures::github_style_schema;
pub(super) use rstest::rstest;
pub(super) use serde_json::Value as JsonValue;

mod conversion;
mod helpers;
mod request_build;
mod unsupported_params;

pub(super) fn cache_write_multiplier_for(retention: CacheRetention) -> rust_decimal::Decimal {
    match retention {
        CacheRetention::None => rust_decimal::Decimal::ONE,
        CacheRetention::Short => rust_decimal::Decimal::new(125, 2),
        CacheRetention::Long => rust_decimal::Decimal::TWO,
    }
}
