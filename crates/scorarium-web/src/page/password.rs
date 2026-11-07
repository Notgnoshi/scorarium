use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::auth::PasswordState;
use serde::Deserialize;

use crate::auth::{Session, SessionCookie};
use crate::error::{Error, Result};
use crate::page::{Base, form_error};
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;

#[derive(Deserialize)]
pub(crate) struct Form {
    new: String,
    confirm: String,
}

#[derive(Template)]
#[template(path = "page/password.html")]
struct Password {
    base: Base,
    error: Option<String>,
}

fn render(header: Header, status: StatusCode, error: Option<String>) -> Result<Response> {
    let page = Password {
        base: Base::new(
            header,
            "Password",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new("Settings", "/settings"),
            ],
        ),
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

async fn require_password(engine: &Engine) -> Result<()> {
    if engine.password_state().await? == PasswordState::NotNeeded {
        return Err(Error::new(
            StatusCode::NOT_FOUND,
            eyre::eyre!("The demo has no password."),
        ));
    }
    Ok(())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
) -> Result<Response> {
    require_password(&engine).await?;
    render(header, StatusCode::OK, None)
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    cookie: SessionCookie,
    header: Header,
    axum::Form(form): axum::Form<Form>,
) -> Result<Response> {
    require_password(&engine).await?;
    if form.confirm != form.new {
        let error = "The passwords do not match.".to_owned();
        return render(header, StatusCode::BAD_REQUEST, Some(error));
    }
    match engine.change_password(&form.new).await {
        Ok(token) => {
            Ok((cookie.set(&token), Redirect::to("/settings?changed=true")).into_response())
        }
        Err(e) => {
            let (status, error) = form_error(e)?;
            render(header, status, Some(error))
        }
    }
}
