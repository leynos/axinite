//! Skills management API handlers.

use std::{error::Error as StdError, sync::Arc};

use axum::{
    Json, Router,
    body::{Body, Bytes, to_bytes},
    extract::{FromRequest, Multipart, Path, Request, State},
    http::{StatusCode, header},
    routing::{delete, get, post},
};
use http_body_util::LengthLimitError;

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::*;
use crate::skills::install_source::{non_blank_raw, trimmed_non_empty};
use crate::skills::registry::{SkillInstallPayload, SkillRegistryError};

const MAX_SKILL_INSTALL_REQUEST_BYTES: usize = 10 * 1024 * 1024;

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/skills", get(skills_list_handler))
        .route("/api/skills/search", post(skills_search_handler))
        .route("/api/skills/install", post(skills_install_handler))
        .route("/api/skills/{name}", delete(skills_remove_handler))
}

pub async fn skills_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<SkillListResponse>, (StatusCode, String)> {
    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    let guard = registry.read().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Skill registry lock poisoned: {}", e),
        )
    })?;

    let skills: Vec<SkillInfo> = guard
        .skills()
        .iter()
        .map(|s| SkillInfo {
            name: s.manifest.name.clone(),
            description: s.manifest.description.clone(),
            version: s.manifest.version.clone(),
            trust: s.trust.to_string(),
            source: format!("{:?}", s.source),
            keywords: s.manifest.activation.keywords.clone(),
        })
        .collect();

    let count = skills.len();
    Ok(Json(SkillListResponse { skills, count }))
}

pub async fn skills_search_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<SkillSearchRequest>,
) -> Result<Json<SkillSearchResponse>, (StatusCode, String)> {
    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    let catalog = state.skill_catalog.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skill catalog not available".to_string(),
    ))?;

    // Search ClawHub catalog
    let catalog_outcome = catalog.search(&req.query).await;
    let catalog_error = catalog_outcome.error.clone();

    // Enrich top results with detail data (stars, downloads, owner)
    let mut entries = catalog_outcome.results;
    catalog.enrich_search_results(&mut entries, 5).await;

    let catalog_json: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "slug": e.slug,
                "name": e.name,
                "description": e.description,
                "version": e.version,
                "score": e.score,
                "updatedAt": e.updated_at,
                "stars": e.stars,
                "downloads": e.downloads,
                "owner": e.owner,
            })
        })
        .collect();

    // Search local skills
    let query_lower = req.query.to_lowercase();
    let installed: Vec<SkillInfo> = {
        let guard = registry.read().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Skill registry lock poisoned: {}", e),
            )
        })?;
        guard
            .skills()
            .iter()
            .filter(|s| {
                s.manifest.name.to_lowercase().contains(&query_lower)
                    || s.manifest.description.to_lowercase().contains(&query_lower)
            })
            .map(|s| SkillInfo {
                name: s.manifest.name.clone(),
                description: s.manifest.description.clone(),
                version: s.manifest.version.clone(),
                trust: s.trust.to_string(),
                source: format!("{:?}", s.source),
                keywords: s.manifest.activation.keywords.clone(),
            })
            .collect()
    };

    Ok(Json(SkillSearchResponse {
        catalog: catalog_json,
        installed,
        registry_url: catalog.registry_url().to_string(),
        catalog_error,
    }))
}

pub async fn skills_install_handler(
    State(state): State<Arc<GatewayState>>,
    request: Request,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let headers = request.headers().clone();

    // Require explicit confirmation header to prevent accidental installs.
    // Chat tools have requires_approval(); this is the equivalent for the web API.
    if headers
        .get("x-confirm-action")
        .and_then(|v| v.to_str().ok())
        != Some("true")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Skill install requires X-Confirm-Action: true header".to_string(),
        ));
    }

    let payload = if is_multipart_request(&headers) {
        payload_from_multipart(request, &state).await?
    } else {
        let req = parse_json_install_request(request).await?;
        payload_from_json_request(&req, &state).await?
    };

    install_skill_payload(&state, payload).await
}

async fn payload_from_json_request(
    req: &SkillInstallRequest,
    state: &GatewayState,
) -> Result<SkillInstallPayload, (StatusCode, String)> {
    let content = non_blank_raw(req.content.as_deref());
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

async fn install_skill_payload(
    state: &GatewayState,
    payload: SkillInstallPayload,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    let install_root = {
        let guard = registry.read().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Skill registry lock poisoned: {}", e),
            )
        })?;
        guard.install_target_dir().to_path_buf()
    };

    let prepared =
        crate::skills::registry::SkillRegistry::prepare_install_to_disk(&install_root, payload)
            .await
            .map_err(map_skill_install_error)?;
    let installed_name = prepared.name().to_string();

    let commit_result = {
        let mut guard = registry.write().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Skill registry lock poisoned: {}", e),
            )
        })?;
        guard.commit_install(prepared)
    };

    match commit_result {
        Ok(()) => Ok(Json(ActionResponse::ok(format!(
            "Skill '{}' installed",
            installed_name
        )))),
        Err(commit_failure) => {
            let (error, prepared) = commit_failure.into_parts();
            if let Err(cleanup_error) =
                crate::skills::registry::SkillRegistry::cleanup_prepared_install(&prepared).await
            {
                tracing::warn!(
                    "failed to cleanup prepared skill install '{}': {}",
                    prepared.name(),
                    cleanup_error
                );
            }
            Ok(Json(ActionResponse::fail(error.to_string())))
        }
    }
}

fn exactly_one_install_source_error() -> (StatusCode, String) {
    (
        StatusCode::BAD_REQUEST,
        "Provide exactly one of 'content', 'url', 'name'/'slug', or a .skill upload".to_string(),
    )
}

fn is_multipart_request(headers: &axum::http::HeaderMap) -> bool {
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

async fn parse_json_install_request(
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

async fn body_bytes(body: Body) -> Result<Bytes, (StatusCode, String)> {
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

async fn payload_from_multipart(
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
            if file_name
                .as_deref()
                .is_some_and(|name| !name.ends_with(".skill"))
            {
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
        "content" => non_blank_raw(Some(text)).is_some(),
        "url" | "name" | "slug" => trimmed_non_empty(Some(text)).is_some(),
        _ => false,
    }
}

fn map_skill_install_error(error: SkillRegistryError) -> (StatusCode, String) {
    if matches!(error, SkillRegistryError::WriteError { .. }) {
        (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    } else {
        (StatusCode::BAD_REQUEST, error.to_string())
    }
}

pub async fn skills_remove_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    // Require explicit confirmation header to prevent accidental removals.
    if headers
        .get("x-confirm-action")
        .and_then(|v| v.to_str().ok())
        != Some("true")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Skill removal requires X-Confirm-Action: true header".to_string(),
        ));
    }

    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    // Validate removal under a brief read lock
    let skill_path = {
        let guard = registry.read().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Skill registry lock poisoned: {}", e),
            )
        })?;
        guard
            .validate_remove(&name)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    };

    // Delete files from disk (async I/O, no lock held)
    crate::skills::registry::SkillRegistry::delete_skill_files(&skill_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Remove from in-memory registry under a brief write lock
    let mut guard = registry.write().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Skill registry lock poisoned: {}", e),
        )
    })?;

    match guard.commit_remove(&name) {
        Ok(()) => Ok(Json(ActionResponse::ok(format!(
            "Skill '{}' removed",
            name
        )))),
        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
    }
}

#[cfg(test)]
mod tests;
