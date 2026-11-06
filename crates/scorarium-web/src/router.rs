use axum::Router;
use scorarium_engine::Engine;
use tower_http::trace::TraceLayer;

pub fn router(engine: Engine) -> Router {
    let trace = TraceLayer::new_for_http()
        .on_request(())
        .on_eos(())
        .on_failure(());
    Router::new().layer(trace).with_state(engine)
}
