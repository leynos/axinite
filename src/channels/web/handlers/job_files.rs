//! Sandbox job project-file handlers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{
    ProjectFileEntry, ProjectFileReadResponse, ProjectFilesResponse,
};

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/jobs/{id}/files/list", get(job_files_list_handler))
        .route("/api/jobs/{id}/files/read", get(job_files_read_handler))
}

#[derive(Deserialize)]
pub struct FilePathQuery {
    pub path: Option<String>,
}

struct ResolvedJobPath {
    requested_path: String,
    canonical_path: std::path::PathBuf,
}

async fn resolve_job_path(
    state: &GatewayState,
    id: &str,
    requested_path: Option<&str>,
    missing_path_message: &'static str,
    missing_target_message: &'static str,
) -> Result<ResolvedJobPath, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let job_id =
        Uuid::parse_str(id).map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let job = store
        .get_sandbox_job(job_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load job: {e}"),
            )
        })?
        .ok_or((StatusCode::NOT_FOUND, "Job not found".to_string()))?;

    let requested_path = requested_path
        .ok_or((StatusCode::BAD_REQUEST, missing_path_message.to_string()))?
        .to_string();

    let base = std::path::PathBuf::from(&job.project_dir);
    let base_canonical = base
        .canonicalize()
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Project dir not found: {e}")))?;
    let target = base.join(&requested_path);
    let canonical_path = target.canonicalize().map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            format!("{missing_target_message}: {e}"),
        )
    })?;
    if !canonical_path.starts_with(&base_canonical) {
        return Err((StatusCode::FORBIDDEN, "Forbidden".to_string()));
    }

    Ok(ResolvedJobPath {
        requested_path,
        canonical_path,
    })
}

pub async fn job_files_list_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<ProjectFilesResponse>, (StatusCode, String)> {
    let resolved = resolve_job_path(
        state.as_ref(),
        &id,
        Some(query.path.as_deref().unwrap_or("")),
        "path parameter required",
        "Path not found",
    )
    .await?;

    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&resolved.canonical_path)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Cannot read directory: {e}")))?;

    while let Some(entry) = read_dir.next_entry().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read directory entry: {e}"),
        )
    })? {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry
            .file_type()
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to read directory entry metadata: {e}"),
                )
            })?
            .is_dir();
        let rel = if resolved.requested_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{name}", resolved.requested_path)
        };
        entries.push(ProjectFileEntry {
            name,
            path: rel,
            is_dir,
        });
    }

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));

    Ok(Json(ProjectFilesResponse { entries }))
}

pub async fn job_files_read_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<ProjectFileReadResponse>, (StatusCode, String)> {
    let resolved = resolve_job_path(
        state.as_ref(),
        &id,
        query.path.as_deref(),
        "path parameter required",
        "File not found",
    )
    .await?;

    let content = tokio::fs::read_to_string(&resolved.canonical_path)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Cannot read file: {e}")))?;

    Ok(Json(ProjectFileReadResponse {
        path: resolved.requested_path,
        content,
    }))
}
