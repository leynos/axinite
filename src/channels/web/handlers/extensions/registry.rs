//! Registry-search handlers for extensions.

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{RegistryEntryInfo, RegistrySearchQuery, RegistrySearchResponse};

pub async fn extensions_registry_handler(
    State(state): State<Arc<GatewayState>>,
    Query(params): Query<RegistrySearchQuery>,
) -> Result<Json<RegistrySearchResponse>, (StatusCode, String)> {
    let query = params.query.unwrap_or_default();
    let query_lower = query.to_lowercase();
    let tokens: Vec<&str> = query_lower.split_whitespace().collect();

    let matching: Vec<&crate::extensions::RegistryEntry> = if tokens.is_empty() {
        state.registry_entries.iter().collect()
    } else {
        state
            .registry_entries
            .iter()
            .filter(|e| {
                let name = e.name.to_lowercase();
                let display = e.display_name.to_lowercase();
                let desc = e.description.to_lowercase();
                tokens.iter().any(|t| {
                    name.contains(t)
                        || display.contains(t)
                        || desc.contains(t)
                        || e.keywords.iter().any(|k| k.to_lowercase().contains(t))
                })
            })
            .collect()
    };

    let installed: HashSet<(String, String)> =
        if let Some(ext_mgr) = state.extension_manager.as_ref() {
            ext_mgr
                .list(None, false)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into_iter()
                .map(|ext| (ext.name, ext.kind.to_string()))
                .collect()
        } else {
            HashSet::new()
        };

    let entries = matching
        .into_iter()
        .map(|e| {
            let kind_str = e.kind.to_string();
            RegistryEntryInfo {
                name: e.name.clone(),
                display_name: e.display_name.clone(),
                installed: installed.contains(&(e.name.clone(), kind_str.clone())),
                kind: kind_str,
                description: e.description.clone(),
                keywords: e.keywords.clone(),
                version: e.version.clone(),
            }
        })
        .collect();

    Ok(Json(RegistrySearchResponse { entries }))
}
