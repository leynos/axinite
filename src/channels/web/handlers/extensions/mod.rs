//! Extension management API route composition.

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};

use crate::channels::web::server::GatewayState;

pub(crate) mod common;
mod install;
mod listing;
mod registry;
mod setup;

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/extensions", get(listing::extensions_list_handler))
        .route(
            "/api/extensions/tools",
            get(listing::extensions_tools_handler),
        )
        .route(
            "/api/extensions/registry",
            get(registry::extensions_registry_handler),
        )
        .route(
            "/api/extensions/install",
            post(install::extensions_install_handler),
        )
        .route(
            "/api/extensions/{name}/activate",
            post(install::extensions_activate_handler),
        )
        .route(
            "/api/extensions/{name}/remove",
            post(install::extensions_remove_handler),
        )
        .route(
            "/api/extensions/{name}/setup",
            get(setup::extensions_setup_handler).post(setup::extensions_setup_submit_handler),
        )
}
