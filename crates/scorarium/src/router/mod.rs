mod assets;
mod import;
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

    pub fn import(library: &db::Library) -> Self {
        Self {
            label: "Import".to_string(),
            href: format!("/library/{}/import", library.id),
        }
    }
}

pub struct BaseContext {
    pub title: String,
    /// The request path, so header links to the current page can be hidden.
    pub path: String,
    pub logged_in: bool,
    /// Imports awaiting review, for the header badge. Zero when logged out.
    pub pending_import_count: i64,
    pub breadcrumbs: Vec<Crumb>,
    pub bootstrap_css: String,
    pub bootstrap_icons_css: String,
    pub bootstrap_js: String,
}

/// The request fills in everything the header needs; the handler adds the title and breadcrumbs with [BaseContext::page]
impl FromRequestParts<Arc<AppState>> for BaseContext {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let logged_in = jar
            .get(SESSION_COOKIE)
            .is_some_and(|cookie| state.sessions.validate(cookie.value()));
        let pending_import_count = if logged_in {
            db::pending_import::count(&state.pool).await?
        } else {
            0
        };
        Ok(Self {
            title: String::new(),
            path: parts.uri.path().to_string(),
            logged_in,
            pending_import_count,
            breadcrumbs: Vec::new(),
            bootstrap_css: assets::url("bootstrap.min.css"),
            bootstrap_icons_css: assets::url("bootstrap-icons.min.css"),
            bootstrap_js: assets::url("bootstrap.bundle.min.js"),
        })
    }
}

impl BaseContext {
    /// Finish the context with what only the handler knows.
    pub fn page(mut self, title: impl Into<String>, breadcrumbs: Vec<Crumb>) -> Self {
        self.title = title.into();
        self.breadcrumbs = breadcrumbs;
        self
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
        .route("/review", get(import::queue))
        .route("/library", post(library::create))
        .route("/library/{id}", get(library::library))
        .route("/library/{id}/rename", post(library::rename))
        .route("/library/{id}/delete", post(library::delete))
        .route(
            "/library/{id}/import",
            get(import::entry).post(import::start),
        )
        .route("/library/{library_id}/import/{id}", get(import::review))
        .route(
            "/library/{library_id}/import/{id}/delete",
            post(import::delete),
        )
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

pub struct AppError(color_eyre::Report);

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
