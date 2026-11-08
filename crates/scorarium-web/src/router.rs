use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Router, middleware};
use scorarium_engine::Engine;
use scorarium_engine::publication::Publication;
use scorarium_engine::work::Work;
use tower_http::trace::TraceLayer;

use crate::auth::CookieSettings;
use crate::{api, assets, error, page};

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
        .route(
            "/library/{id}/publication/{publication}/edit",
            get(page::library::publication_edit::get).post(page::library::publication_edit::post),
        )
        .route(
            "/library/{id}/publication/{publication}/delete",
            post(page::library::publication_edit::delete),
        )
        .route("/library/{id}/work/{work}", get(page::library::work::get))
        .route(
            "/library/{id}/work/{work}/edit",
            get(page::library::work_edit::get).post(page::library::work_edit::post),
        )
        .route(
            "/library/{id}/work/{work}/delete",
            post(page::library::work_edit::delete),
        )
        .route(
            "/library/{id}/person/{person}",
            get(page::library::person::get),
        )
        .route(
            "/library/{id}/person/{person}/edit",
            get(page::library::person_edit::get).post(page::library::person_edit::post),
        )
        .route(
            "/library/{id}/person/{person}/delete",
            post(page::library::person_edit::delete),
        )
        .route("/library/{id}/people", get(page::library::people::get))
        .route(
            "/library/{id}/people/{role}",
            get(page::library::people::get),
        )
        .route("/library/{id}/tags", get(page::library::tags::get))
        .route("/library/{id}/tag/{tag}", get(page::library::tag::get))
        .route(
            "/library/{id}/edit",
            get(page::library::edit::get).post(page::library::edit::post),
        )
        .route("/library/{id}/delete", post(page::library::edit::delete))
        .route("/search", get(page::search::get))
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
        // API routes render errors as fragments for htmx to swap in.
        .nest(
            "/api",
            Router::new()
                .route("/search", get(api::search::get))
                .route("/library/{id}/suggest/{field}", get(api::suggest::get))
                .route(
                    "/library/{id}/work/{work}/contributor/add",
                    post(api::contributor::add::<Work>),
                )
                .route(
                    "/library/{id}/work/{work}/contributor/remove",
                    post(api::contributor::remove::<Work>),
                )
                .route(
                    "/library/{id}/publication/{publication}/contributor/add",
                    post(api::contributor::add::<Publication>),
                )
                .route(
                    "/library/{id}/publication/{publication}/contributor/remove",
                    post(api::contributor::remove::<Publication>),
                )
                .fallback(|| async { StatusCode::NOT_FOUND })
                .layer(middleware::from_fn(error::render_fragment)),
        )
        .layer(trace)
        .layer(Extension(CookieSettings {
            secure: secure_cookies,
        }))
        .with_state(engine)
}
