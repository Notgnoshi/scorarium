use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::auth::PasswordState;
use serde::Deserialize;

use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;

#[derive(Deserialize)]
pub(crate) struct Query {
    changed: Option<bool>,
}

#[derive(Template)]
#[template(path = "page/settings.html")]
struct Settings {
    base: Base,
    changed: bool,
    password: bool,
}

pub async fn get(
    State(engine): State<Engine>,
    _session: Session,
    header: Header,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Html<String>> {
    let page = Settings {
        base: Base::new(header, "Settings", vec![Crumb::new("Libraries", "/")]),
        changed: query.changed.unwrap_or(false),
        password: engine.password_state().await? != PasswordState::NotNeeded,
    };
    Ok(Html(page.render()?))
}
