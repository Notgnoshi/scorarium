use axum::extract::{FromRequest, RawForm, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use crate::error::Error;

/// Two views of one submitted form
///
/// A page's own fields and a widget's fields arrive in one body. Deserializing each struct from it
/// keeps the widget's field names in the widget's module.
pub(crate) struct Forms<A, B>(pub A, pub B);

impl<S, A, B> FromRequest<S> for Forms<A, B>
where
    S: Send + Sync,
    A: DeserializeOwned,
    B: DeserializeOwned,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let RawForm(body) = RawForm::from_request(req, state)
            .await
            .map_err(|e| Error::new(e.status(), e))?;
        let unprocessable = |e| Error::new(StatusCode::UNPROCESSABLE_ENTITY, e);
        let a = serde_html_form::from_bytes(&body).map_err(unprocessable)?;
        let b = serde_html_form::from_bytes(&body).map_err(unprocessable)?;
        Ok(Forms(a, b))
    }
}
