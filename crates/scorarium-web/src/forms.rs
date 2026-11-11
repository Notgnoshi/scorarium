use axum::body::Bytes;
use axum::extract::{FromRequest, RawForm, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use crate::error::Error;

/// The body of a submitted form
///
/// A page's fields and each widget's fields arrive together in one body.
/// Each owner reads its own field struct.
pub(crate) struct FormBody(Bytes);

impl<S: Send + Sync> FromRequest<S> for FormBody {
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let RawForm(body) = RawForm::from_request(req, state)
            .await
            .map_err(|e| Error::new(e.status(), e))?;
        Ok(Self(body))
    }
}

impl FormBody {
    /// Read the fields that `T` owns, ignoring the rest
    pub fn read<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_html_form::from_bytes(&self.0)
            .map_err(|e| Error::new(StatusCode::UNPROCESSABLE_ENTITY, e))
    }
}
