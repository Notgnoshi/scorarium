mod assets;
mod index;
mod library;
mod login;
mod password;
mod person;
mod publication;
mod work;

use std::sync::Arc;

use axum::Router;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum_extra::extract::CookieJar;
use tower_http::trace::TraceLayer;

use crate::{AppState, db};

/// The name of the cookie holding the login session token.
const SESSION_COOKIE: &str = "session";

pub struct Crumb {
    pub label: String,
    pub href: String,
}

impl Crumb {
    pub fn home() -> Self {
        Self {
            label: "Home".to_string(),
            href: "/".to_string(),
        }
    }

    pub fn library(library: &db::Library) -> Self {
        Self {
            label: library.name.clone(),
            href: format!("/library/{}", library.id),
        }
    }
}

pub struct BaseContext {
    pub title: String,
    /// The request path, so header links to the current page can be hidden.
    pub path: String,
    pub logged_in: bool,
    pub breadcrumbs: Vec<Crumb>,
    pub bootstrap_css: String,
}

impl BaseContext {
    pub fn new(
        title: impl Into<String>,
        path: impl Into<String>,
        logged_in: bool,
        breadcrumbs: Vec<Crumb>,
    ) -> Self {
        Self {
            title: title.into(),
            path: path.into(),
            logged_in,
            breadcrumbs,
            bootstrap_css: assets::url("bootstrap.min.css"),
        }
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index::index))
        .route("/assets/{*name}", get(assets::asset))
        .route("/login", get(login::login_form).post(login::login))
        .route("/logout", post(login::logout))
        .route(
            "/password",
            get(password::password_form).post(password::change_password),
        )
        .route("/library", post(library::create))
        .route("/library/{id}", get(library::library))
        .route("/library/{id}/rename", post(library::rename))
        .route("/library/{id}/delete", post(library::delete))
        .route(
            "/library/{library_id}/publication/{id}",
            get(publication::publication),
        )
        .route("/library/{library_id}/work/{id}", get(work::work))
        .route("/library/{library_id}/person/{id}", get(person::person))
        .route("/library/{id}/composers", get(person::composers))
        .route("/library/{id}/authors", get(person::authors))
        .with_state(state)
        // Applies only to the routes added above it, so keep this last.
        .layer(TraceLayer::new_for_http())
}

/// Does this request's cookie belong to a live session?
fn logged_in(state: &AppState, jar: &CookieJar) -> bool {
    jar.get(SESSION_COOKIE)
        .is_some_and(|cookie| state.sessions.validate(cookie.value()))
}

/// The session token of a logged-in request.
struct Session(String);

impl FromRequestParts<Arc<AppState>> for Session {
    type Rejection = Redirect;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        match jar.get(SESSION_COOKIE) {
            Some(cookie) if state.sessions.validate(cookie.value()) => {
                Ok(Session(cookie.value().to_string()))
            }
            _ => Err(Redirect::to("/login")),
        }
    }
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
