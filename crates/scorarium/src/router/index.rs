use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::response::Html;
use axum_extra::extract::CookieJar;
use sqlx::SqlitePool;

use super::{AppError, BaseContext};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    base: BaseContext,
    libraries: Vec<db::Library>,
    error: Option<&'static str>,
}

pub async fn index(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Html<String>, AppError> {
    render(&state.pool, super::logged_in(&state, &jar), None).await
}

pub(super) async fn render(
    pool: &SqlitePool,
    logged_in: bool,
    error: Option<&'static str>,
) -> Result<Html<String>, AppError> {
    let page = IndexPage {
        base: BaseContext::new("Libraries", "/", logged_in, Vec::new()),
        libraries: db::list_libraries(pool).await?,
        error,
    };
    Ok(Html(page.render()?))
}
