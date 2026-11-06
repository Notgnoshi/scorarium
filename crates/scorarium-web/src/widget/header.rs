use askama::Template;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use scorarium_engine::Engine;

use crate::error::Error;

/// The navbar at the top of every page
#[derive(Debug, Default, Template)]
#[template(path = "widget/header.html")]
pub struct Header {
    demo: bool,
}

impl FromRequestParts<Engine> for Header {
    type Rejection = Error;

    async fn from_request_parts(_parts: &mut Parts, engine: &Engine) -> Result<Self, Error> {
        Ok(Header {
            demo: engine.is_demo(),
        })
    }
}

impl askama::filters::HtmlSafe for Header {}
