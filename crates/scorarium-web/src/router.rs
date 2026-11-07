use axum::routing::{get, post};
use axum::{Extension, Router, middleware};
use scorarium_engine::Engine;
use tower_http::trace::TraceLayer;

use crate::auth::CookieSettings;
use crate::{assets, error, page};

/// The name of the cookie that carries the session token
pub const SESSION_COOKIE: &str = "session";

pub fn router(engine: Engine, secure_cookies: bool) -> Router {
    let trace = TraceLayer::new_for_http()
        .on_request(())
        .on_eos(())
        .on_failure(());
    Router::new()
        .route("/", get(page::index::get))
        .route("/library", post(page::index::post))
        .route("/library/{id}", get(page::library::view::get))
        .route(
            "/library/{id}/publication/{publication}",
            get(page::library::publication::get),
        )
        .route("/library/{id}/work/{work}", get(page::library::work::get))
        .route(
            "/library/{id}/edit",
            get(page::library::edit::get).post(page::library::edit::post),
        )
        .route("/library/{id}/delete", post(page::library::edit::delete))
        .route("/login", get(page::login::get).post(page::login::post))
        .route("/logout", post(page::logout::post))
        .route("/settings", get(page::settings::get))
        .route(
            "/settings/password",
            get(page::password::get).post(page::password::post),
        )
        .route("/settings/audit", get(page::audit::get))
        .route("/assets/{name}", get(assets::get))
        .fallback(error::not_found)
        .method_not_allowed_fallback(error::method_not_allowed)
        .layer(middleware::from_fn_with_state(
            engine.clone(),
            error::render,
        ))
        .layer(trace)
        .layer(Extension(CookieSettings {
            secure: secure_cookies,
        }))
        .with_state(engine)
}
