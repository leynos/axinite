//! Skills management API handlers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::*;
use crate::skills::registry::{SkillInstallPayload, SkillRegistryError};

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
    headers: axum::http::HeaderMap,
    Json(req): Json<SkillInstallRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
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

    let registry = state.skill_registry.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Skills system not enabled".to_string(),
    ))?;

    let payload = if let Some(ref raw) = req.content {
        SkillInstallPayload::Markdown(raw.clone())
    } else if let Some(ref url) = req.url {
        // Fetch from explicit URL (with SSRF protection)
        crate::tools::builtin::skill_fetch::fetch_skill_bytes(url)
            .await
            .map(SkillInstallPayload::DownloadedBytes)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    } else if let Some(ref catalog) = state.skill_catalog {
        // Prefer slug (e.g. "owner/skill-name") over display name for the
        // download URL, since the registry endpoint expects a slug.
        let download_key = req
            .slug
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&req.name);
        let url = crate::skills::catalog::skill_download_url(catalog.registry_url(), download_key);
        crate::tools::builtin::skill_fetch::fetch_skill_bytes(&url)
            .await
            .map(SkillInstallPayload::DownloadedBytes)
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
    } else {
        return Ok(Json(ActionResponse::fail(
            "Provide 'content' or 'url' to install a skill".to_string(),
        )));
    };

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
