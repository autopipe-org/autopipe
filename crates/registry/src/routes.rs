use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::db::DbState;
use crate::handlers;
use crate::web;

pub fn create_router(state: Arc<DbState>) -> Router {
    Router::new()
        // Web pages (browser)
        .route("/", get(web::index_page))
        .route("/pipelines/{id}", get(web::detail_page))
        .route("/pipelines/{id}/download", get(web::download_zip))
        // REST API (MCP server)
        .route("/api/pipelines", get(handlers::list_or_search).post(handlers::create_pipeline))
        .route(
            "/api/pipelines/{id}",
            get(handlers::get_pipeline)
                .put(handlers::update_pipeline)
                .delete(handlers::delete_pipeline),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}
