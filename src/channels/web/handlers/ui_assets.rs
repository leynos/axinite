//! Embedded browser UI assets and variant selection.
//!
//! Serves the SolidJS single-page app (default) or the legacy handwritten
//! shell, both embedded at compile time.

use std::sync::Arc;

use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};

use crate::channels::web::handlers::static_files::health_handler;
use crate::channels::web::server::GatewayState;

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
