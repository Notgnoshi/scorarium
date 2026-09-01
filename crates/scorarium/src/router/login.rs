use std::sync::Arc;

use axum::Form;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::Deserialize;

use super::{AppError, SESSION_COOKIE};
use crate::auth::PasswordCheck;
use crate::{AppState, auth, db, session};

/// GET /login
pub async fn login_form(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    if super::logged_in(&state, &jar) {
        // already logged in; redirect to index
        return Ok(Redirect::to("/").into_response());
    }
    let claimed = db::get_password_hash(&state.pool).await?.is_some();
    let page = if claimed {
        // the initial password has been set; show the login form
        login_page(None)
    } else {
        // no password has been set; assume the first login attempt is the admin user
        claim_page(None)
    };
    Ok(Html(page).into_response())
}

#[derive(Deserialize)]
pub struct LoginForm {
    password: String,
    /// Present only on submissions of the claim form.
    confirm: Option<String>,
}

/// POST /login - handle results of the login form
pub async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    // Re-check the claim state on every POST because the form the browser rendered may be stale
    let claimed = db::get_password_hash(&state.pool).await?.is_some();
    let page = match (claimed, form.confirm) {
        (false, Some(confirm)) => {
            if form.password.is_empty() {
                claim_page(Some("The password must not be empty."))
            } else if confirm != form.password {
                claim_page(Some("The passwords did not match."))
            } else if auth::claim_password(&state.pool, &form.password).await? {
                return Ok(start_session(&state, jar));
            } else {
                // Lost a race against a concurrent claim
                login_page(Some("A password was already set. Log in with it."))
            }
        }
        (false, None) => claim_page(Some("No password is set yet. Set one first.")),
        (true, Some(_)) => login_page(Some("A password is already set. Log in with it.")),
        (true, None) => match auth::verify_password(&state.pool, &form.password).await? {
            PasswordCheck::Correct => return Ok(start_session(&state, jar)),
            PasswordCheck::Wrong | PasswordCheck::Unclaimed => login_page(Some("Login failed")),
        },
    };
    Ok(Html(page).into_response())
}

pub async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        state.sessions.revoke(cookie.value());
    }
    // The removal cookie's path must match the path the cookie was set with
    let removal = Cookie::build((SESSION_COOKIE, "")).path("/");
    (jar.remove(removal), Redirect::to("/")).into_response()
}

/// Start a session and hand its token to the browser.
fn start_session(state: &AppState, jar: CookieJar) -> Response {
    let token = state.sessions.create();
    let max_age = cookie::time::Duration::seconds(session::SESSION_LIFETIME.as_secs() as i64);
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(max_age);
    (jar.add(cookie), Redirect::to("/")).into_response()
}

fn login_page(error: Option<&str>) -> String {
    let error = error.map(|e| format!("<p>{e}</p>\n")).unwrap_or_default();
    format!(
        "<!doctype html>\n\
         <title>Log in</title>\n\
         <h1>Log in</h1>\n\
         {error}\
         <form method=\"post\" action=\"/login\">\n\
         <label>Password <input type=\"password\" name=\"password\" required></label>\n\
         <button>Log in</button>\n\
         </form>\n"
    )
}

fn claim_page(error: Option<&str>) -> String {
    let error = error.map(|e| format!("<p>{e}</p>\n")).unwrap_or_default();
    format!(
        "<!doctype html>\n\
         <title>Set password</title>\n\
         <h1>Set password</h1>\n\
         <p>No password is set yet. The first login sets it.</p>\n\
         {error}\
         <form method=\"post\" action=\"/login\">\n\
         <label>Password <input type=\"password\" name=\"password\" required></label>\n\
         <label>Confirm <input type=\"password\" name=\"confirm\" required></label>\n\
         <button>Set password</button>\n\
         </form>\n"
    )
}
