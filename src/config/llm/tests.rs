//! Unit tests for LLM backend and model resolution from the environment.

mod backend_resolution;
mod cache_retention;
mod extra_headers;
mod model_selection;
mod oauth;

/// Clear all openai-compatible-related env vars.
fn clear_openai_compatible_env() {
    // SAFETY: Only called under ENV_MUTEX in tests.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
        std::env::remove_var("LLM_BASE_URL");
        std::env::remove_var("LLM_MODEL");
    }
}
