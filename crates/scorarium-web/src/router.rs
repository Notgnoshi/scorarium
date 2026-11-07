use axum::routing::get;
use axum::{Router, middleware};
use scorarium_engine::Engine;
use tower_http::trace::TraceLayer;

use crate::{assets, error, page};

/// The name of the cookie that carries the session token
pub const SESSION_COOKIE: &str = "session";

pub fn router(engine: Engine) -> Router {
    let trace = TraceLayer::new_for_http()
        .on_request(())
        .on_eos(())
        .on_failure(());
    Router::new()
        .route("/", get(page::index::get))
        .route("/assets/{name}", get(assets::get))
        .fallback(error::not_found)
        .method_not_allowed_fallback(error::method_not_allowed)
        .layer(middleware::from_fn_with_state(
            engine.clone(),
            error::render,
        ))
        .layer(trace)
        .with_state(engine)
}
