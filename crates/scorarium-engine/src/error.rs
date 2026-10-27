pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The entity no longer exists
    #[error("not found")]
    NotFound,
    /// A value that failed validation
    #[error("{0}")]
    InvalidValue(String),
    /// Generic catch-all error for cases that don't need to be distinguished
    #[error(transparent)]
    Generic(#[from] eyre::Report),
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Error::Generic(e.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Generic(e.into())
    }
}
