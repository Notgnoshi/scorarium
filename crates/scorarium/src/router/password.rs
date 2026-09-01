use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, Session};
use crate::auth::PasswordCheck;
use crate::{AppState, auth};

/// GET /password
pub async fn password_form(_session: Session) -> Result<Response, AppError> {
    Ok(no_store(password_page(None)?))
}

#[derive(Deserialize)]
pub struct PasswordForm {
    current: String,
    new: String,
    confirm: String,
}

/// POST /password - handle results of the change-password form
pub async fn change_password(
    Session(token): Session,
    State(state): State<Arc<AppState>>,
    Form(form): Form<PasswordForm>,
) -> Result<Response, AppError> {
    if form.new != form.confirm {
        return Ok(no_store(password_page(Some(
            "The new passwords did not match.",
        ))?));
    }
    if form.new.is_empty() {
        return Ok(no_store(password_page(Some(
            "The new password must not be empty.",
        ))?));
    }
    match auth::verify_password(&state.pool, &form.current).await? {
        PasswordCheck::Correct => {}
        PasswordCheck::Wrong | PasswordCheck::Unclaimed => {
            return Ok(no_store(password_page(Some("Wrong current password."))?));
        }
    }
    auth::change_password(&state.pool, &form.new).await?;
    // Changing the password is how a possibly-compromised session gets locked out, so end every
    // session other than the one that made the change.
    state.sessions.revoke_all_except(&token);
    Ok(Redirect::to("/").into_response())
}

/// Keep pages shown only to logged-in users out of browser caches
fn no_store(page: String) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Html(page)).into_response()
}

#[derive(Template)]
#[template(path = "password.html")]
struct PasswordPage {
    base: BaseContext,
    error: Option<&'static str>,
}

fn password_page(error: Option<&'static str>) -> askama::Result<String> {
    PasswordPage {
        base: BaseContext::new("Change password", "/password", true, vec![Crumb::home()]),
        error,
    }
    .render()
}
