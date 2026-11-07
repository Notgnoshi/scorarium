use askama::Template;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use scorarium_engine::Engine;

use crate::auth::{self, Viewer};
use crate::error::Error;

/// The navbar at the top of every page
#[derive(Debug, Default, Template)]
#[template(path = "widget/header.html")]
pub struct Header {
    demo: bool,
    logged_in: bool,
    login_url: String,
}

impl FromRequestParts<Engine> for Header {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, engine: &Engine) -> Result<Self, Error> {
        let Ok(viewer) = Viewer::from_request_parts(parts, engine).await;
        Ok(Header {
            demo: engine.is_demo(),
            logged_in: viewer.is_logged_in(),
            login_url: auth::login_url(&parts.method, &parts.uri),
        })
    }
}

impl askama::filters::HtmlSafe for Header {}
