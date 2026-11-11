use std::fmt;

use axum::body::Bytes;
use axum::extract::{FromRequest, RawForm, Request};
use axum::http::StatusCode;
use serde::Serialize;
use serde::de::{DeserializeOwned, Deserializer, SeqAccess, Visitor};
use serde_qs::{ArrayFormat, Config};

use crate::error::Error;

// browsers percent-encodes the brackets in a field's name.
const CONFIG: Config = Config::new()
    .use_form_encoding(true)
    .array_format(ArrayFormat::Unindexed);

/// Read a form from the bytes of a body or a query string
pub(crate) fn from_bytes<T: DeserializeOwned>(input: &[u8]) -> Result<T, serde_qs::Error> {
    CONFIG.deserialize_bytes(input)
}

/// Write `form` as a query string
pub(crate) fn to_string<T: Serialize>(form: &T) -> Result<String, serde_qs::Error> {
    CONFIG.serialize_string(form)
}

/// Read every value of a repeated name, keeping a lone blank one
pub(crate) fn repeated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    struct Values;

    impl<'de> Visitor<'de> for Values {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("the values of a repeated name")
        }

        // serde_qs presents one blank value as a unit.
        fn visit_unit<E>(self) -> Result<Vec<String>, E> {
            Ok(vec![String::new()])
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<String>, E> {
            Ok(vec![value.to_owned()])
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                values.push(value);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_any(Values)
}

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
        from_bytes(&self.0).map_err(|e| Error::new(StatusCode::UNPROCESSABLE_ENTITY, e))
    }
}
