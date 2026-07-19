//! Static asset, logs, and gateway-status handlers.

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, put},
};
use tokio_stream::StreamExt;

use crate::bootstrap::axinite_base_dir;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::*;

/// Which browser implementation the gateway serves at `/`.
///
/// The SolidJS single-page app (built from `web-src/` into
/// `src/channels/web/static/solid/`) is the default. The legacy handwritten
/// shell remains embedded purely as an operator rollback path during the
/// migration (RFC 0018 Stage 3) and is selected by setting
/// `AXINITE_WEB_UI=legacy` before startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiVariant {
    Solid,
    Legacy,
}

/// Resolve the UI variant from the `AXINITE_WEB_UI` environment variable.
pub fn ui_variant() -> UiVariant {
    match std::env::var("AXINITE_WEB_UI") {
        Ok(value) if value.eq_ignore_ascii_case("legacy") => UiVariant::Legacy,
        _ => UiVariant::Solid,
    }
}

/// Paths handled client-side by the SolidJS router. Each must serve the app
/// shell so deep links and reloads work.
const SOLID_APP_ROUTES: &[&str] = &[
    "/",
    "/chat",
    "/memory",
    "/jobs",
    "/routines",
    "/extensions",
    "/skills",
];

/// Embedded Fluent locale bundles for the SolidJS app, keyed by locale code.
const SOLID_LOCALES: &[(&str, &str)] = &[
    ("ar", include_str!("../static/solid/locales/ar/common.ftl")),
    ("de", include_str!("../static/solid/locales/de/common.ftl")),
    (
        "en-GB",
        include_str!("../static/solid/locales/en-GB/common.ftl"),
    ),
    ("fr", include_str!("../static/solid/locales/fr/common.ftl")),
    ("hi", include_str!("../static/solid/locales/hi/common.ftl")),
    ("it", include_str!("../static/solid/locales/it/common.ftl")),
    ("ja", include_str!("../static/solid/locales/ja/common.ftl")),
    ("nl", include_str!("../static/solid/locales/nl/common.ftl")),
    ("pl", include_str!("../static/solid/locales/pl/common.ftl")),
    (
        "zh-CN",
        include_str!("../static/solid/locales/zh-CN/common.ftl"),
    ),
];

pub fn public_routes() -> Router<Arc<GatewayState>> {
    routes_for(ui_variant())
}

/// Build the public asset routes for the given UI variant.
pub fn routes_for(variant: UiVariant) -> Router<Arc<GatewayState>> {
    match variant {
        UiVariant::Solid => {
            let mut router = Router::new();
            for path in SOLID_APP_ROUTES {
                router = router.route(path, get(solid_index_handler));
            }
            router
                .route("/assets/app.js", get(solid_js_handler))
                .route("/assets/index.css", get(solid_css_handler))
                .route("/assets/axinite32.ico", get(solid_icon_handler))
                .route("/favicon.ico", get(solid_icon_handler))
                .route("/locales/{locale}/common.ftl", get(solid_locale_handler))
                .route("/api/health", get(health_handler))
        }
        UiVariant::Legacy => Router::new()
            .route("/", get(index_handler))
            .route("/style.css", get(css_handler))
            .route("/app.js", get(js_handler))
            .route("/favicon.ico", get(favicon_handler))
            .route("/api/health", get(health_handler)),
    }
}

pub async fn solid_index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/solid/index.html"),
    )
}

pub async fn solid_js_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/solid/assets/app.js"),
    )
}

pub async fn solid_css_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/solid/assets/index.css"),
    )
}

pub async fn solid_icon_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/x-icon"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../static/solid/assets/axinite32.ico").as_slice(),
    )
}

pub async fn solid_locale_handler(Path(locale): Path<String>) -> axum::response::Response {
    match SOLID_LOCALES
        .iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(&locale))
    {
        Some((_, bundle)) => (
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            *bundle,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Unknown locale").into_response(),
    }
}

pub fn protected_routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/logs/events", get(logs_events_handler))
        .route("/api/logs/level", get(logs_level_get_handler))
        .route("/api/logs/level", put(logs_level_set_handler))
        .route("/api/gateway/status", get(gateway_status_handler))
        .route("/projects/{project_id}", get(project_redirect_handler))
        .route("/projects/{project_id}/", get(project_index_handler))
        .route("/projects/{project_id}/{*path}", get(project_file_handler))
}

pub async fn index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/index.html"),
    )
}

pub async fn css_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/style.css"),
    )
}

pub async fn js_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/app.js"),
    )
}

pub async fn favicon_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/x-icon"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../static/favicon.ico").as_slice(),
    )
}

pub async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy",
        channel: "gateway",
    })
}

pub async fn project_redirect_handler(Path(project_id): Path<String>) -> impl IntoResponse {
    axum::response::Redirect::permanent(&format!("/projects/{project_id}/"))
}

pub async fn project_index_handler(Path(project_id): Path<String>) -> impl IntoResponse {
    serve_project_file(&project_id, "index.html").await
}

pub async fn project_file_handler(
    Path((project_id, path)): Path<(String, String)>,
) -> impl IntoResponse {
    serve_project_file(&project_id, &path).await
}

/// Rejects `project_id` values that could escape the projects directory.
fn is_invalid_project_id(project_id: &str) -> bool {
    let has_path_separator = project_id.contains('/') || project_id.contains('\\');
    let is_traversal_or_empty = project_id.contains("..") || project_id.is_empty();
    has_path_separator || is_traversal_or_empty
}

async fn serve_project_file(project_id: &str, path: &str) -> axum::response::Response {
    if is_invalid_project_id(project_id) {
        return (StatusCode::BAD_REQUEST, "Invalid project ID").into_response();
    }

    let base = axinite_base_dir().join("projects").join(project_id);

    let file_path = base.join(path);

    // Path traversal guard
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    if !canonical.starts_with(&base_canonical) {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    match tokio::fs::read(&canonical).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(&canonical)
                .first_or_octet_stream()
                .to_string();
            ([(header::CONTENT_TYPE, mime)], contents).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

pub async fn logs_events_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let broadcaster = state.log_broadcaster.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Log broadcaster not available".to_string(),
    ))?;

    // Replay recent history so late-joining browsers see startup logs.
    // Subscribe BEFORE snapshotting to avoid a gap between history and live.
    let rx = broadcaster.subscribe();
    let history = broadcaster.recent_entries();

    let history_stream = futures::stream::iter(history).map(|entry| {
        let data = serde_json::to_string(&entry).unwrap_or_default();
        Ok::<_, Infallible>(Event::default().event("log").data(data))
    });

    let live_stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|result| result.ok())
        .map(|entry| {
            let data = serde_json::to_string(&entry).unwrap_or_default();
            Ok::<_, Infallible>(Event::default().event("log").data(data))
        });

    let stream = history_stream.chain(live_stream);

    Ok((
        [("X-Accel-Buffering", "no"), ("Cache-Control", "no-cache")],
        Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(30))
                .text(""),
        ),
    ))
}

pub async fn logs_level_get_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let handle = state.log_level_handle.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Log level control not available".to_string(),
    ))?;
    Ok(Json(serde_json::json!({ "level": handle.current_level() })))
}

pub async fn logs_level_set_handler(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let handle = state.log_level_handle.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Log level control not available".to_string(),
    ))?;

    let level = body
        .get("level")
        .and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "missing 'level' field".to_string()))?;

    handle
        .set_level(level)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    tracing::info!("Log level changed to '{}'", handle.current_level());
    Ok(Json(serde_json::json!({ "level": handle.current_level() })))
}

pub async fn gateway_status_handler(
    State(state): State<Arc<GatewayState>>,
) -> Json<GatewayStatusResponse> {
    let sse_connections = state.sse.connection_count();
    let ws_connections = state
        .ws_tracker
        .as_ref()
        .map(|t| t.connection_count())
        .unwrap_or(0);

    let uptime_secs = state.startup_time.elapsed().as_secs();

    let (daily_cost, actions_this_hour, model_usage) = if let Some(ref cg) = state.cost_guard {
        let cost = cg.daily_spend().await;
        let actions = cg.actions_this_hour().await;
        let usage = cg.model_usage().await;
        let models: Vec<ModelUsageEntry> = usage
            .into_iter()
            .map(|(model, tokens)| ModelUsageEntry {
                model,
                input_tokens: tokens.input_tokens,
                output_tokens: tokens.output_tokens,
                cost: format!("{:.6}", tokens.cost),
            })
            .collect();
        (Some(format!("{:.4}", cost)), Some(actions), Some(models))
    } else {
        (None, None, None)
    };

    let restart_enabled = std::env::var("AXINITE_IN_DOCKER")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    Json(GatewayStatusResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        sse_connections,
        ws_connections,
        total_connections: sse_connections + ws_connections,
        uptime_secs,
        restart_enabled,
        daily_cost,
        actions_this_hour,
        model_usage,
    })
}

#[derive(serde::Serialize)]
struct ModelUsageEntry {
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost: String,
}

#[derive(serde::Serialize)]
pub struct GatewayStatusResponse {
    version: String,
    sse_connections: u64,
    ws_connections: u64,
    total_connections: u64,
    uptime_secs: u64,
    restart_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    daily_cost: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actions_this_hour: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_usage: Option<Vec<ModelUsageEntry>>,
}

#[cfg(test)]
mod tests {
    //! Unit tests for UI variant selection and embedded asset serving.

    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::channels::web::test_helpers::TestGatewayBuilder;

    fn app(variant: UiVariant) -> Router {
        routes_for(variant).with_state(TestGatewayBuilder::new().build())
    }

    async fn get_path(variant: UiVariant, path: &str) -> (StatusCode, String, String) {
        let response = app(variant)
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap_or_default().to_string())
            .unwrap_or_default();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            content_type,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    #[tokio::test]
    async fn solid_variant_serves_spa_shell_at_root_and_app_routes() {
        for path in [
            "/",
            "/chat",
            "/memory",
            "/jobs",
            "/routines",
            "/extensions",
            "/skills",
        ] {
            let (status, content_type, body) = get_path(UiVariant::Solid, path).await;
            assert_eq!(status, StatusCode::OK, "path {path}");
            assert!(content_type.starts_with("text/html"), "path {path}");
            assert!(body.contains("id=\"app\""), "SPA mount missing for {path}");
            assert!(
                body.contains("/assets/app.js"),
                "bundle ref missing for {path}"
            );
        }
    }

    #[tokio::test]
    async fn solid_variant_serves_stable_asset_names() {
        let (status, content_type, body) = get_path(UiVariant::Solid, "/assets/app.js").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "application/javascript");
        assert!(!body.is_empty());

        let (status, content_type, _) = get_path(UiVariant::Solid, "/assets/index.css").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "text/css");
    }

    #[tokio::test]
    async fn solid_variant_serves_locale_bundles() {
        let (status, content_type, body) =
            get_path(UiVariant::Solid, "/locales/en-GB/common.ftl").await;
        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/plain"));
        assert!(body.contains("route-chat-label"));

        let (status, _, _) = get_path(UiVariant::Solid, "/locales/xx/common.ftl").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn legacy_variant_still_serves_the_handwritten_shell() {
        let (status, content_type, body) = get_path(UiVariant::Legacy, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/html"));
        assert!(body.contains("app.js"));

        let (status, _, _) = get_path(UiVariant::Legacy, "/chat").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn ui_variant_defaults_to_solid() {
        // Note: reads the real process environment; AXINITE_WEB_UI is not
        // set in the test environment.
        assert_eq!(ui_variant(), UiVariant::Solid);
    }
}
