//! Shared parsing helpers for skill installation handlers.

use std::{error::Error as StdError, sync::Arc};

use axum::{
    body::{Body, Bytes, to_bytes},
    extract::{FromRequest, Multipart, Request},
    http::{StatusCode, header},
};
use http_body_util::LengthLimitError;

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::SkillInstallRequest;
use crate::skills::install_source::{non_blank_raw, trimmed_non_empty as trimmed_non_empty_raw};
use crate::skills::registry::{SkillInstallPayload, SkillRegistryError};

pub(crate) const MAX_SKILL_INSTALL_REQUEST_BYTES: usize = 10 * 1024 * 1024;

pub(crate) async fn select_json_install_source(
    req: &SkillInstallRequest,
    state: &GatewayState,
) -> Result<SkillInstallPayload, (StatusCode, String)> {
    let content = non_blank(req.content.as_deref());
    let url = trimmed_non_empty(req.url.as_deref());
    let catalog_key =
        trimmed_non_empty(req.slug.as_deref()).or_else(|| trimmed_non_empty(req.name.as_deref()));

    let mut selected = 0;
    if content.is_some() {
        selected += 1;
    }
    if url.is_some() {
        selected += 1;
    }
    if catalog_key.is_some() {
        selected += 1;
    }

    if selected != 1 {
        return Err(exactly_one_install_source_error());
    }

    if let Some(raw) = content {
        return Ok(SkillInstallPayload::Markdown(raw.to_string()));
    }

    if let Some(url) = url {
        return crate::tools::builtin::skill_fetch::fetch_skill_bytes(url)
            .await
            .map(SkillInstallPayload::DownloadedBytes)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()));
    }

    let catalog_key = catalog_key.ok_or_else(exactly_one_install_source_error)?;
    let catalog = state.skill_catalog.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skill catalog not available".to_string(),
    ))?;
    let url = crate::skills::catalog::skill_download_url(catalog.registry_url(), catalog_key);
    crate::tools::builtin::skill_fetch::fetch_skill_bytes(&url)
        .await
        .map(SkillInstallPayload::DownloadedBytes)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))
}

pub(crate) fn non_blank(value: Option<&str>) -> Option<&str> {
    non_blank_raw(value)
}

pub(crate) fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    trimmed_non_empty_raw(value)
}

pub(crate) fn is_multipart_request(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|media_type| media_type.eq_ignore_ascii_case("multipart/form-data"))
        })
}

pub(crate) async fn parse_json_install_request(
    request: Request,
) -> Result<SkillInstallRequest, (StatusCode, String)> {
    let body = body_bytes(request.into_body()).await?;
    serde_json::from_slice(&body).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid JSON body: {error}"),
        )
    })
}

pub(crate) async fn body_bytes(body: Body) -> Result<Bytes, (StatusCode, String)> {
    to_bytes(body, MAX_SKILL_INSTALL_REQUEST_BYTES)
        .await
        .map_err(map_body_read_error)
}

fn map_body_read_error(error: axum::Error) -> (StatusCode, String) {
    if StdError::source(&error).is_some_and(|source| source.is::<LengthLimitError>()) {
        (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "Request body exceeds maximum size of {} bytes",
                MAX_SKILL_INSTALL_REQUEST_BYTES
            ),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to read request body: {error}"),
        )
    }
}

pub(crate) async fn payload_from_multipart(
    request: Request,
    state: &Arc<GatewayState>,
) -> Result<SkillInstallPayload, (StatusCode, String)> {
    let mut multipart = Multipart::from_request(request, state)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let mut upload: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().map(str::to_string);
        let contents = field
            .bytes()
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;

        if name == "bundle" {
            if upload.is_some() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Provide exactly one .skill upload".to_string(),
                ));
            }
            let Some(file_name) = file_name else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Uploaded skill bundle must include a filename ending with .skill".to_string(),
                ));
            };
            if !file_name.ends_with(".skill") {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Uploaded skill bundle filename must end with .skill".to_string(),
                ));
            }
            upload = Some(contents.to_vec());
        } else if multipart_source_field_has_value(&name, &contents) {
            return Err(exactly_one_install_source_error());
        }
    }

    if let Some(bytes) = upload {
        Ok(SkillInstallPayload::ArchiveBytes(bytes))
    } else {
        Err(exactly_one_install_source_error())
    }
}

fn multipart_source_field_has_value(name: &str, contents: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(contents) else {
        return matches!(name, "content" | "url" | "name" | "slug") && !contents.is_empty();
    };

    match name {
        "content" => non_blank(Some(text)).is_some(),
        "url" | "name" | "slug" => trimmed_non_empty(Some(text)).is_some(),
        _ => false,
    }
}

fn exactly_one_install_source_error() -> (StatusCode, String) {
    (
        StatusCode::BAD_REQUEST,
        "Provide exactly one of 'content', 'url', 'name'/'slug', or a .skill upload".to_string(),
    )
}

pub(crate) fn map_skill_install_error(error: SkillRegistryError) -> (StatusCode, String) {
    if matches!(error, SkillRegistryError::WriteError { .. }) {
        (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    } else {
        (StatusCode::BAD_REQUEST, error.to_string())
    }
}
