pub mod db;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

/// Shared state for all request handlers.
pub struct AppState {
    pub pool: SqlitePool,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(hello))
        .with_state(state)
        // Applies only to the routes added above it, so keep this last.
        .layer(TraceLayer::new_for_http())
}

async fn hello() -> &'static str {
    "scorarium"
}
