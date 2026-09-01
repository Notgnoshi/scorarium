pub mod auth;
pub mod db;
pub mod session;

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

/// Shared state for all request handlers.
pub struct AppState {
    pub pool: SqlitePool,
    pub sessions: session::SessionStore,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            sessions: session::SessionStore::default(),
        }
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .with_state(state)
        // Applies only to the routes added above it, so keep this last.
        .layer(TraceLayer::new_for_http())
}

struct AppError(color_eyre::Report);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(error = ?self.0, "handler error");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

impl<E: Into<color_eyre::Report>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

async fn index(State(state): State<Arc<AppState>>) -> Result<String, AppError> {
    let libraries = db::list_libraries(&state.pool).await?;
    Ok(libraries.into_iter().fold(String::new(), |mut body, lib| {
        body.push_str(&lib.name);
        body.push('\n');
        body
    }))
}
