use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::auth::PasswordState;
use serde::Deserialize;

use crate::auth::{SessionCookie, Viewer};
use crate::error::Result;
use crate::page::{Base, form_error};
use crate::widget::header::Header;

#[derive(Deserialize)]
pub(crate) struct Query {
    next: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct Form {
    password: String,
    confirm: Option<String>,
    next: Option<String>,
}

#[derive(Template)]
#[template(path = "page/login.html")]
struct Login {
    base: Base,
    state: PasswordState,
    next: String,
    error: Option<String>,
}

/// Keep `next` only when it stays on this site, otherwise go home
fn safe_next(next: Option<String>) -> String {
    match next {
        // Browsers parse `\` as `/` and strip tabs and newlines in URLs, so `/\evil.com` and
        // `/<TAB>/evil.com` resolve to another site just as `//evil.com` does.
        Some(next)
            if next.starts_with('/')
                && !next[1..].starts_with(['/', '\\'])
                && !next.contains(|c: char| c.is_ascii_control()) =>
        {
            next
        }
        _ => "/".to_owned(),
    }
}

fn render(
    header: Header,
    state: PasswordState,
    next: String,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let title = match state {
        PasswordState::Unclaimed => "Set a password",
        PasswordState::Claimed | PasswordState::NotNeeded => "Log in",
    };
    let page = Login {
        base: Base::new(header, title, vec![]),
        state,
        next,
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    State(engine): State<Engine>,
    viewer: Viewer,
    header: Header,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Response> {
    let next = safe_next(query.next);
    if viewer.is_logged_in() {
        return Ok(Redirect::to(&next).into_response());
    }
    let state = engine.password_state().await?;
    render(header, state, next, StatusCode::OK, None)
}

pub async fn post(
    State(engine): State<Engine>,
    cookie: SessionCookie,
    header: Header,
    axum::Form(form): axum::Form<Form>,
) -> Result<Response> {
    let next = safe_next(form.next);
    let state = engine.password_state().await?;
    let result = match state {
        PasswordState::Unclaimed => {
            if form.confirm.as_deref() != Some(form.password.as_str()) {
                let error = "The passwords do not match.".to_owned();
                return render(header, state, next, StatusCode::BAD_REQUEST, Some(error));
            }
            match engine.replace_password(&form.password).await {
                Ok(()) => engine.login(&form.password).await,
                Err(e) => Err(e),
            }
        }
        PasswordState::Claimed => engine.login(&form.password).await,
        PasswordState::NotNeeded => engine.login("").await,
    };
    match result {
        Ok(token) => Ok((cookie.set(&token), Redirect::to(&next)).into_response()),
        Err(e) => {
            let (status, error) = form_error(e)?;
            render(header, state, next, status, Some(error))
        }
    }
}
