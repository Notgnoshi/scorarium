use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum_extra::extract::CookieJar;

use super::AppError;
use crate::{AppState, db};

pub async fn index(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Html<String>, AppError> {
    let libraries = db::list_libraries(&state.pool).await?;
    let items = libraries.into_iter().fold(String::new(), |mut items, lib| {
        items.push_str("<li>");
        items.push_str(&html_escape::encode_text(&lib.name));
        items.push_str("</li>\n");
        items
    });
    let auth_controls = if super::logged_in(&state, &jar) {
        "<a href=\"/password\">Change password</a>\n\
         <form method=\"post\" action=\"/logout\">\n\
         <button>Log out</button>\n\
         </form>\n"
    } else {
        "<a href=\"/login\">Log in</a>\n"
    };
    Ok(Html(format!(
        "<!doctype html>\n\
         <title>Scorarium</title>\n\
         <h1>Libraries</h1>\n\
         <ul>\n\
         {items}\
         </ul>\n\
         {auth_controls}"
    )))
}
