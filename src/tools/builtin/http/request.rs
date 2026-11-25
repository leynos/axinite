//! Request and response steps for the HTTP tool's `execute`: method dispatch,
//! body attachment, credential injection, size limits and saving to disk.

use futures::StreamExt;
use reqwest::{Client, RequestBuilder, Response};

use crate::tools::tool::ToolError;
use crate::tools::wasm::{InjectedCredentials, inject_credential};

use super::HttpTool;
use super::validation::{declared_content_length, validate_save_to_path};

/// Start a request for one of the supported HTTP methods.
pub(super) fn request_builder(
    client: &Client,
    method: &str,
    url: &reqwest::Url,
) -> Result<RequestBuilder, ToolError> {
    match method.to_uppercase().as_str() {
        "GET" => Ok(client.get(url.clone())),
        "POST" => Ok(client.post(url.clone())),
        "PUT" => Ok(client.put(url.clone())),
        "DELETE" => Ok(client.delete(url.clone())),
        "PATCH" => Ok(client.patch(url.clone())),
        _ => Err(ToolError::InvalidParameters(format!(
            "unsupported method: {}",
            method
        ))),
    }
}

/// Attach the optional body, returning the request and the body bytes for leak scanning.
///
/// A string body that parses as JSON is sent as JSON; any other string is sent as-is.
pub(super) fn attach_body(
    request: RequestBuilder,
    body: Option<&serde_json::Value>,
) -> Result<(RequestBuilder, Option<Vec<u8>>), ToolError> {
    let Some(body) = body else {
        return Ok((request, None));
    };
    let invalid =
        |e: serde_json::Error| ToolError::InvalidParameters(format!("invalid body JSON: {}", e));
    let Some(body_str) = body.as_str() else {
        let bytes = serde_json::to_vec(body).map_err(invalid)?;
        return Ok((request.json(body), Some(bytes)));
    };
    match serde_json::from_str::<serde_json::Value>(body_str) {
        Ok(json_body) => {
            let bytes = serde_json::to_vec(&json_body).map_err(invalid)?;
            Ok((request.json(&json_body), Some(bytes)))
        }
        Err(_) => Ok((
            request.body(body_str.to_string()),
            Some(body_str.as_bytes().to_vec()),
        )),
    }
}

impl HttpTool {
    /// Inject the shared registry's credentials for the URL's host into the request.
    ///
    /// Injected headers are also appended to `headers_vec` and injected query
    /// parameters to `parsed_url`, so the leak scan sees what is sent.
    pub(super) async fn inject_registry_credentials(
        &self,
        user_id: &str,
        request: RequestBuilder,
        target: &mut RequestTarget<'_>,
    ) -> RequestBuilder {
        let host = target.url.host_str().unwrap_or("").to_string();
        let injected = self.registry_credentials(user_id, &host).await;
        apply_credentials(request, target, &injected)
    }

    /// Decrypt every credential the shared registry maps to `host`, skipping failures.
    async fn registry_credentials(&self, user_id: &str, host: &str) -> InjectedCredentials {
        let mut all = InjectedCredentials::empty();
        let (Some(registry), Some(store)) = (
            self.credential_registry.as_ref(),
            self.secrets_store.as_ref(),
        ) else {
            return all;
        };

        let matched: Vec<crate::secrets::CredentialMapping> = registry.find_for_host(host);
        for mapping in &matched {
            match store.get_decrypted(user_id, &mapping.secret_name).await {
                Ok(secret) => {
                    let mut injected = InjectedCredentials::empty();
                    inject_credential(&mut injected, &mapping.location, &secret);
                    all.headers.extend(injected.headers);
                    all.query_params.extend(injected.query_params);
                }
                Err(e) => {
                    tracing::warn!(
                        secret = %mapping.secret_name,
                        error = %e,
                        "Failed to inject credential for HTTP tool"
                    );
                }
            }
        }
        all
    }
}

/// The URL and header list a request is built from, kept in step with injected credentials.
pub(super) struct RequestTarget<'a> {
    pub(super) url: &'a mut reqwest::Url,
    pub(super) headers: &'a mut Vec<(String, String)>,
}

/// Add injected headers and query parameters to the request and mirror them in the target.
pub(super) fn apply_credentials(
    mut request: RequestBuilder,
    target: &mut RequestTarget<'_>,
    injected: &InjectedCredentials,
) -> RequestBuilder {
    for (name, value) in &injected.headers {
        request = request.header(name.as_str(), value.as_str());
        target.headers.push((name.clone(), value.clone()));
    }
    for (name, value) in &injected.query_params {
        target.url.query_pairs_mut().append_pair(name, value);
        request = request.query(&[(name.as_str(), value.as_str())]);
    }
    request
}

/// Reject a response whose declared Content-Length exceeds the limit, before downloading it.
///
/// This prevents OOM from malicious servers.
pub(super) fn reject_oversized_declared_length(
    response: &Response,
    url: &reqwest::Url,
    max_size: usize,
) -> Result<(), ToolError> {
    if let Some(len) = declared_content_length(response)
        && len > max_size
    {
        tracing::warn!(
            url = %url,
            content_length = len,
            max = max_size,
            "Rejected HTTP response: Content-Length exceeds limit"
        );
        return Err(ToolError::ExecutionFailed(format!(
            "Response Content-Length ({} bytes) exceeds maximum allowed size ({} bytes)",
            len, max_size
        )));
    }
    Ok(())
}

/// Stream the response body with a hard size cap.
///
/// Even if Content-Length was absent or lied about the size, reading stops
/// once the limit is exceeded.
pub(super) async fn read_capped_body(
    response: Response,
    max_size: usize,
) -> Result<bytes::Bytes, ToolError> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|e| {
            ToolError::ExternalService(format!("failed to read response body: {}", e))
        })?;
        if body.len() + chunk.len() > max_size {
            return Err(ToolError::ExecutionFailed(format!(
                "Response body exceeds maximum allowed size ({} bytes)",
                max_size
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(bytes::Bytes::from(body))
}

/// Write the raw response bytes to `save_to` after validating the path.
pub(super) async fn save_body_to_file(
    save_to: &str,
    body_bytes: &bytes::Bytes,
) -> Result<(), ToolError> {
    let save_to_owned = save_to.to_string();
    let bytes_clone = body_bytes.clone();
    tokio::task::spawn_blocking(move || {
        let canonical = validate_save_to_path(&save_to_owned)?;
        ambient_fs::write(&canonical, &bytes_clone)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write file: {}", e)))?;
        Ok::<_, ToolError>(canonical)
    })
    .await
    .map_err(|e| ToolError::ExecutionFailed(format!("spawn_blocking failed: {}", e)))?
    .map_err(|e: ToolError| e)?;
    Ok(())
}
