//! Shared helpers for web handlers: error mapping and logged failure responses.

use axum::http::StatusCode;

use crate::channels::web::types::ActionResponse;

pub(crate) fn internal_error(
    context: &'static str,
    error: impl std::fmt::Display,
) -> (StatusCode, String) {
    tracing::error!(error = %error, "{context}");
    (StatusCode::INTERNAL_SERVER_ERROR, context.to_string())
}

pub(crate) fn logged_failure(
    message: String,
    context: &'static str,
    error: impl std::fmt::Display,
) -> ActionResponse {
    tracing::error!(error = %error, "{context}");
    ActionResponse::fail(message)
}
