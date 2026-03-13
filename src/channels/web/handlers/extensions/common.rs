//! Shared helpers for extension-management handlers.

use crate::channels::web::types::ActionResponse;

pub async fn maybe_extension_auth_url(
    ext_mgr: &crate::extensions::ExtensionManager,
    name: &str,
) -> Option<String> {
    match ext_mgr.auth(name, None).await {
        Ok(auth_result) if auth_result.auth_url().is_some() => {
            auth_result.auth_url().map(String::from)
        }
        _ => None,
    }
}

pub async fn activation_required_response(
    ext_mgr: &crate::extensions::ExtensionManager,
    name: &str,
    fallback_message: String,
    auth_error_context: String,
) -> ActionResponse {
    match ext_mgr.auth(name, None).await {
        Ok(auth_result) => {
            let mut resp = ActionResponse::fail(
                auth_result
                    .instructions()
                    .map(String::from)
                    .unwrap_or(fallback_message),
            );
            resp.auth_url = auth_result.auth_url().map(String::from);
            resp.awaiting_token = Some(auth_result.is_awaiting_token());
            resp.instructions = auth_result.instructions().map(String::from);
            resp
        }
        Err(auth_err) => ActionResponse::fail(format!("{auth_error_context}: {auth_err}")),
    }
}
