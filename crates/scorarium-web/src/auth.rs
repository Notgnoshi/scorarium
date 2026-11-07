use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Method, Uri};
use axum_extra::extract::CookieJar;
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

pub struct SessionCookie {
    jar: CookieJar,
}

impl FromRequestParts<Engine> for SessionCookie {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, Infallible> {
        let jar = CookieJar::from_request_parts(parts, engine).await?;
        Ok(SessionCookie { jar })
    }
}

impl SessionCookie {
    fn token(&self) -> Option<Token> {
        self.jar
            .get(SESSION_COOKIE)
            .map(|cookie| Token::from(cookie.value()))
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
