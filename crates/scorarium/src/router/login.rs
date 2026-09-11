use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use scorarium_archive::PasswordCheck;
use serde::Deserialize;

use super::{AppError, BackQuery, BaseContext, Crumb, SESSION_COOKIE, back_or};
use crate::{AppState, session};

/// GET /login
pub async fn login_form(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Query(query): Query<BackQuery>,
) -> Result<Response, AppError> {
    if base.logged_in {
        // already logged in; go where the login would have led
        return Ok(Redirect::to(&back_or(query.back, "/".to_string())).into_response());
    }
    // The demo has no password to claim or check; its login form is just a button
    if state.demo {
        return Ok(Html(login_page(base, None, query.back)?).into_response());
    }
    let claimed = state.archive.password_claimed().await?;
    let page = if claimed {
        // the initial password has been set; show the login form
        login_page(base, None, query.back)?
    } else {
        // no password has been set; assume the first login attempt is the admin user
        claim_page(base, None, query.back)?
    };
    Ok(Html(page).into_response())
}

#[derive(Deserialize)]
pub struct LoginForm {
    #[serde(default)]
    password: String,
    /// Present only on submissions of the claim form.
    confirm: Option<String>,
    /// Where to go once logged in
    back: Option<String>,
}

/// POST /login - handle results of the login form
pub async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    base: BaseContext,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let next = back_or(form.back.clone(), "/".to_string());
    // A stale form from a tab opened before logging in elsewhere
    if base.logged_in {
        return Ok(Redirect::to(&next).into_response());
    }
    if state.demo {
        return Ok(start_session(&state, jar, &next));
    }
    // Re-check the claim state on every POST because the form the browser rendered may be stale
    let claimed = state.archive.password_claimed().await?;
    let page = match (claimed, form.confirm) {
        (false, Some(confirm)) => {
            if form.password.is_empty() {
                claim_page(base, Some("The password must not be empty."), form.back)?
            } else if confirm != form.password {
                claim_page(base, Some("The passwords did not match."), form.back)?
            } else if state.archive.claim_password(&form.password).await? {
                return Ok(start_session(&state, jar, &next));
            } else {
                // Lost a race against a concurrent claim
                login_page(
                    base,
                    Some("A password was already set. Log in with it."),
                    form.back,
                )?
            }
        }
        (false, None) => claim_page(
            base,
            Some("No password is set yet. Set one first."),
            form.back,
        )?,
        (true, Some(_)) => login_page(
            base,
            Some("A password is already set. Log in with it."),
            form.back,
        )?,
        (true, None) => match state.archive.verify_password(&form.password).await? {
            PasswordCheck::Correct => return Ok(start_session(&state, jar, &next)),
            PasswordCheck::Wrong | PasswordCheck::Unclaimed => {
                login_page(base, Some("Login failed"), form.back)?
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

/// Start a session, hand its token to the browser, and send it on to `next`.
fn start_session(state: &AppState, jar: CookieJar, next: &str) -> Response {
    let token = state.sessions.create();
    let max_age = cookie::time::Duration::seconds(session::SESSION_LIFETIME.as_secs() as i64);
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .secure(state.secure_cookies)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(max_age);
    (jar.add(cookie), Redirect::to(next)).into_response()
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage {
    base: BaseContext,
    error: Option<&'static str>,
    back: Option<String>,
}

#[derive(Template)]
#[template(path = "claim.html")]
struct ClaimPage {
    base: BaseContext,
    error: Option<&'static str>,
    back: Option<String>,
}

fn login_page(
    base: BaseContext,
    error: Option<&'static str>,
    back: Option<String>,
) -> askama::Result<String> {
    LoginPage {
        base: base.page("Log in", vec![Crumb::home()]),
        error,
        back,
    }
    .render()
}

fn claim_page(
    base: BaseContext,
    error: Option<&'static str>,
    back: Option<String>,
) -> askama::Result<String> {
    ClaimPage {
        base: base.page("Set password", vec![Crumb::home()]),
        error,
        back,
    }
    .render()
}
