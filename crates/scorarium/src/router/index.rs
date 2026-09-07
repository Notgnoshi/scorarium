use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_archive::Library;

use super::{AppError, BaseContext};
use crate::AppState;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    base: BaseContext,
    libraries: Vec<Library>,
    error: Option<&'static str>,
}

pub async fn index(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
) -> Result<Html<String>, AppError> {
    render(&state, base, None).await
}

pub(super) async fn render(
    state: &AppState,
    base: BaseContext,
    error: Option<&'static str>,
) -> Result<Html<String>, AppError> {
    let page = IndexPage {
        base: base.page("Libraries", Vec::new()),
        libraries: state.archive.libraries().await?,
        error,
    };
    Ok(Html(page.render()?))
}
