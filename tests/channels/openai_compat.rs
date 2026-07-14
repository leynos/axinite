//! Integration tests for the OpenAI-compatible API endpoints.
//!
//! Uses a mock LLM provider so no real API key is needed.
//! Shared mocks live in `helpers`; the tests are grouped into
//! `completions` (happy-path behaviour) and `validation`
//! (input validation, auth, and limits).

#[path = "openai_compat/helpers.rs"]
mod helpers;

#[path = "openai_compat/completions.rs"]
mod completions;
#[path = "openai_compat/validation.rs"]
mod validation;
