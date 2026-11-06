use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

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

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let Error { status, report } = self;
        if status.is_server_error() {
            let chain = report.chain().map(ToString::to_string).collect::<Vec<_>>();
            tracing::error!(status = %status, error = %chain.join(": "));
        }
        (status, report.to_string()).into_response()
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
