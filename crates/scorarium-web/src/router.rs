use axum::Router;
use axum::routing::get;
use scorarium_engine::Engine;
use tower_http::trace::TraceLayer;

use crate::{assets, error};

pub fn router(engine: Engine) -> Router {
    let trace = TraceLayer::new_for_http()
        .on_request(())
        .on_eos(())
        .on_failure(());
    Router::new()
        .route("/assets/{name}", get(assets::get))
        .fallback(error::not_found)
        .method_not_allowed_fallback(error::method_not_allowed)
        .layer(trace)
        .with_state(engine)
}
