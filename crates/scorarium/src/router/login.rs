use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, SESSION_COOKIE};
use crate::auth::PasswordCheck;
use crate::{AppState, auth, db, session};

/// GET /login
pub async fn login_form(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
) -> Result<Response, AppError> {
    if base.logged_in {
        // already logged in; redirect to index
        return Ok(Redirect::to("/").into_response());
    }
    let claimed = db::get_password_hash(&state.pool).await?.is_some();
    let page = if claimed {
        // the initial password has been set; show the login form
        login_page(base, None)?
    } else {
        // no password has been set; assume the first login attempt is the admin user
        claim_page(base, None)?
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
    base: BaseContext,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    // A stale form from a tab opened before logging in elsewhere
    if base.logged_in {
        return Ok(Redirect::to("/").into_response());
    }
    // Re-check the claim state on every POST because the form the browser rendered may be stale
    let claimed = db::get_password_hash(&state.pool).await?.is_some();
    let page = match (claimed, form.confirm) {
        (false, Some(confirm)) => {
            if form.password.is_empty() {
                claim_page(base, Some("The password must not be empty."))?
            } else if confirm != form.password {
                claim_page(base, Some("The passwords did not match."))?
            } else if auth::claim_password(&state.pool, &form.password).await? {
                return Ok(start_session(&state, jar));
            } else {
                // Lost a race against a concurrent claim
                login_page(base, Some("A password was already set. Log in with it."))?
            }
        }
        (false, None) => claim_page(base, Some("No password is set yet. Set one first."))?,
        (true, Some(_)) => login_page(base, Some("A password is already set. Log in with it."))?,
        (true, None) => match auth::verify_password(&state.pool, &form.password).await? {
            PasswordCheck::Correct => return Ok(start_session(&state, jar)),
            PasswordCheck::Wrong | PasswordCheck::Unclaimed => {
                login_page(base, Some("Login failed"))?
            }
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

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage {
    base: BaseContext,
    error: Option<&'static str>,
}

#[derive(Template)]
#[template(path = "claim.html")]
struct ClaimPage {
    base: BaseContext,
    error: Option<&'static str>,
}

fn login_page(base: BaseContext, error: Option<&'static str>) -> askama::Result<String> {
    LoginPage {
        base: base.page("Log in", vec![Crumb::home()]),
        error,
    }
    .render()
}

fn claim_page(base: BaseContext, error: Option<&'static str>) -> askama::Result<String> {
    ClaimPage {
        base: base.page("Set password", vec![Crumb::home()]),
        error,
    }
    .render()
}
