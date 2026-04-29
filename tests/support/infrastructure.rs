//! Support modules compiled only for the `infrastructure` harness.

/// Test utilities for webhook-related infrastructure tests, including helper
/// servers, request builders, and assertion helpers used by this harness.
#[path = "webhook_common.rs"]
pub mod webhook_helpers;
