//! LLM provider chaos tests (QA Plan item 4.1).
//!
//! Tests the failover chain, circuit breaker, and retry logic under realistic
//! failure modes with specialized mock providers.
//!
//! Mock providers live in `providers`; the tests are grouped into
//! `retry_and_breaker` and `failover`.

#[path = "provider_chaos/providers.rs"]
mod providers;

#[path = "provider_chaos/failover.rs"]
mod failover;
#[path = "provider_chaos/retry_and_breaker.rs"]
mod retry_and_breaker;
