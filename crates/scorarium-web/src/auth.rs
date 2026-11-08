use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use scorarium_engine::Engine;
use scorarium_engine::auth::Token;

use crate::SESSION_COOKIE;

/// The characters askama's `urlencode` filter escapes, so that `/` stays readable in an encoded URL
const URL_ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'.')
    .remove(b'-')
    .remove(b'~')
    .remove(b'/');

#[derive(Clone, Copy)]
pub(crate) struct CookieSettings {
    pub(crate) secure: bool,
}

pub struct SessionCookie {
    jar: CookieJar,
    secure: bool,
}

impl FromRequestParts<Engine> for SessionCookie {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, Infallible> {
        let jar = CookieJar::from_request_parts(parts, engine).await?;
        let secure = parts
            .extensions
            .get::<CookieSettings>()
            .is_none_or(|settings| settings.secure);
        Ok(SessionCookie { jar, secure })
    }
}

impl SessionCookie {
    pub fn set(self, token: &Token) -> CookieJar {
        // SameSite=Lax withholds the cookie from cross-site POSTs, which protects every form that
        // relies on the session from CSRF.
        let cookie = Cookie::build((SESSION_COOKIE, token.as_str().to_owned()))
            .http_only(true)
            .same_site(SameSite::Lax)
            .path("/")
            .secure(self.secure);
        self.jar.add(cookie)
    }

    pub fn token(&self) -> Option<Token> {
        self.jar
            .get(SESSION_COOKIE)
            .map(|cookie| Token::from(cookie.value()))
    }

    pub fn clear(self) -> CookieJar {
        self.jar.remove(Cookie::build(SESSION_COOKIE).path("/"))
    }
}

pub enum Viewer {
    Anonymous,
    LoggedIn,
}

impl Viewer {
    pub fn is_logged_in(&self) -> bool {
        matches!(self, Viewer::LoggedIn)
    }
}

impl FromRequestParts<Engine> for Viewer {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, Infallible> {
        let cookie = SessionCookie::from_request_parts(parts, engine).await?;
        // A missing, expired, or revoked token is anonymous rather than an error.
        match cookie.token() {
            Some(token) if engine.is_authenticated(&token) => Ok(Viewer::LoggedIn),
            _ => Ok(Viewer::Anonymous),
        }
    }
}

/// Build the login URL that returns to `uri` afterwards, for a GET only
pub fn login_url(method: &Method, uri: &Uri) -> String {
    // Returning to a path that only accepts POST would land on a 405.
    if method != Method::GET {
        return "/login".to_owned();
    }
    let next = uri.path_and_query().map_or(uri.path(), |pq| pq.as_str());
    format!("/login?next={}", utf8_percent_encode(next, URL_ENCODE))
}

/// A live session, which a page takes to require login
///
/// Extracting it from a logged-out request redirects to the login page.
pub struct Session;

impl FromRequestParts<Engine> for Session {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, Redirect> {
        let Ok(viewer) = Viewer::from_request_parts(parts, engine).await;
        if viewer.is_logged_in() {
            Ok(Session)
        } else {
            Err(login_redirect(&parts.method, &parts.uri))
        }
    }
}

/// A live session for an `/api` route
///
/// Extracting it from a logged-out request rejects with a 401 carrying `HX-Redirect`, which htmx
/// uses to redirect to the login page.
pub struct ApiSession;

impl FromRequestParts<Engine> for ApiSession {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, Response> {
        let Ok(viewer) = Viewer::from_request_parts(parts, engine).await;
        if viewer.is_logged_in() {
            return Ok(ApiSession);
        }
        let current = parts
            .headers
            .get("HX-Current-URL")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Uri>().ok());
        // The current URL is a page the browser is showing, so returning to it is always a GET,
        // whatever the method of the API request.
        let url = current.map_or_else(|| "/login".to_owned(), |uri| login_url(&Method::GET, &uri));
        Err((StatusCode::UNAUTHORIZED, [("HX-Redirect", url)]).into_response())
    }
}

/// Build the redirect to the login page that returns to `uri` afterwards
pub fn login_redirect(method: &Method, uri: &Uri) -> Redirect {
    Redirect::to(&login_url(method, uri))
}
