use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::response::Html;
use axum_extra::extract::CookieJar;

use super::{AppError, BaseContext};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    base: BaseContext,
    libraries: Vec<db::Library>,
}

pub async fn index(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Html<String>, AppError> {
    let page = IndexPage {
        base: BaseContext::new("Libraries", "/", super::logged_in(&state, &jar), Vec::new()),
        libraries: db::list_libraries(&state.pool).await?,
    };
    Ok(Html(page.render()?))
}
