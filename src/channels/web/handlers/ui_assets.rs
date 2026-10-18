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
use crate::config::EnvContext;

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

/// Environment variable that selects the served browser UI.
const UI_VARIANT_ENV: &str = "AXINITE_WEB_UI";

/// Resolve the UI variant from the ambient `AXINITE_WEB_UI` environment
/// variable.
pub fn ui_variant() -> UiVariant {
    ui_variant_from(&EnvContext::capture_ambient())
}

/// Resolve the UI variant from an explicit environment snapshot.
///
/// `legacy` (case-insensitively) selects the handwritten shell; anything else,
/// including an unset variable, selects the SolidJS app.
pub fn ui_variant_from(env: &EnvContext) -> UiVariant {
    match env.get(UI_VARIANT_ENV) {
        Some(value) if value.eq_ignore_ascii_case("legacy") => UiVariant::Legacy,
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
    "/logs",
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

/// Build the public asset routes for the UI variant selected by
/// `AXINITE_WEB_UI` at startup.
pub fn public_routes() -> Router<Arc<GatewayState>> {
    let variant = ui_variant();
    let ui_variant = match variant {
        UiVariant::Solid => "solid",
        UiVariant::Legacy => "legacy",
    };
    tracing::info!(ui_variant, "Selected gateway browser UI");
    routes_for(variant)
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

/// Serve the embedded SolidJS shell as UTF-8 HTML for app routes.
/// Uses `Cache-Control: no-cache` to avoid retaining an old shell.
pub async fn solid_index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/solid/index.html"),
    )
}

/// Serve embedded SolidJS JavaScript as `application/javascript`.
/// Uses `Cache-Control: no-cache` to avoid retaining an old bundle.
pub async fn solid_js_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/solid/assets/app.js"),
    )
}

/// Serve the embedded SolidJS stylesheet as `text/css`.
/// Uses `Cache-Control: no-cache` to avoid retaining old styles.
pub async fn solid_css_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/solid/assets/index.css"),
    )
}

/// Serve the embedded SolidJS favicon as `image/x-icon`.
/// The icon may be cached for one day.
pub async fn solid_icon_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/x-icon"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../static/solid/assets/axinite32.ico").as_slice(),
    )
}

/// Serve a case-insensitive Fluent locale as UTF-8 `text/plain`.
/// Known bundles use `Cache-Control: no-cache`; unknown locales return `404`.
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

/// Serve the embedded legacy UI shell as UTF-8 HTML.
/// Uses `Cache-Control: no-cache` to avoid retaining an old shell.
pub async fn index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/index.html"),
    )
}

/// Serve the embedded legacy stylesheet as `text/css`.
/// Uses `Cache-Control: no-cache` to avoid retaining old styles.
pub async fn css_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/style.css"),
    )
}

/// Serve embedded legacy JavaScript as `application/javascript`.
/// Uses `Cache-Control: no-cache` to avoid retaining an old bundle.
pub async fn js_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../static/app.js"),
    )
}

/// Serve the embedded legacy favicon as `image/x-icon`.
/// The icon may be cached for one day.
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
mod tests;
