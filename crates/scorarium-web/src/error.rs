use askama::Template;
use axum::Extension;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;

use crate::page::Base;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    status: StatusCode,
    report: eyre::Report,
}

impl Error {
    pub fn new(status: StatusCode, report: impl Into<eyre::Report>) -> Error {
        Error {
            status,
            report: report.into(),
        }
    }
}

impl From<scorarium_engine::Error> for Error {
    fn from(e: scorarium_engine::Error) -> Self {
        use scorarium_engine::Error as E;
        match e {
            E::NotFound => Error::new(StatusCode::NOT_FOUND, e),
            E::InvalidValue(_) | E::WrongPassword => Error::new(StatusCode::BAD_REQUEST, e),
            E::Throttled { .. } => Error::new(StatusCode::TOO_MANY_REQUESTS, e),
            // Keep the report as-is so its chain is not wrapped a second time.
            E::Generic(report) => Error::new(StatusCode::INTERNAL_SERVER_ERROR, report),
        }
    }
}

impl From<askama::Error> for Error {
    fn from(e: askama::Error) -> Self {
        Error::new(StatusCode::INTERNAL_SERVER_ERROR, e)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let Error { status, report } = self;
        if status.is_server_error() {
            let chain = report.chain().map(ToString::to_string).collect::<Vec<_>>();
            tracing::error!(status = %status, error = %chain.join(": "));
        }
        let message = report.to_string();
        (status, Extension(ErrorDetail { message })).into_response()
    }
}

/// A tag on a response that tells the [`render`] middleware to replace it with an error page
#[derive(Clone, Debug)]
pub struct ErrorDetail {
    pub message: String,
}

#[derive(Template)]
#[template(path = "page/error.html")]
struct ErrorPage {
    base: Base,
    status: StatusCode,
    message: String,
}

/// Replace a response tagged with [`ErrorDetail`] by the rendered error page
pub async fn render(State(engine): State<Engine>, request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    let response = next.run(Request::from_parts(parts.clone(), body)).await;
    let Some(ErrorDetail { message }) = response.extensions().get().cloned() else {
        return response;
    };

    let status = response.status();
    let header = Header::from_request_parts(&mut parts, &engine)
        .await
        .unwrap_or_default();
    let title = format!(
        "{} {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    );
    let page = ErrorPage {
        base: Base::new(header, title, vec![Crumb::new("Libraries", "/")]), // TODO: Remove crumb once there's another usage of it to make the dead_code lint go away
        status,
        message,
    };
    match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to render the error page");
            (status, page.message).into_response()
        }
    }
}

pub async fn not_found(uri: Uri) -> Error {
    Error::new(StatusCode::NOT_FOUND, eyre::eyre!("no page at {uri}"))
}

pub async fn method_not_allowed(method: Method, uri: Uri) -> Error {
    Error::new(
        StatusCode::METHOD_NOT_ALLOWED,
        eyre::eyre!("{method} is not allowed on {uri}"),
    )
}
