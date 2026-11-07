pub mod index;
pub mod library;
pub mod login;
pub mod logout;
pub mod password;
pub mod settings;

use axum::http::StatusCode;

use crate::error::Result;
use crate::widget::breadcrumbs::{Breadcrumbs, Crumb};
use crate::widget::header::Header;

/// Context for rendering the base.html page template
#[derive(Debug)]
pub struct Base {
    pub header: Header,
    pub title: String,
    pub breadcrumbs: Breadcrumbs,
}

impl Base {
    /// Build the context for a base page
    ///
    /// The title is both the document title and the active breadcrumb.
    pub fn new(header: Header, title: impl Into<String>, parents: Vec<Crumb>) -> Base {
        let title = title.into();
        Base {
            header,
            breadcrumbs: Breadcrumbs::new(parents, title.clone()),
            title,
        }
    }
}

/// Map an engine error that a form shows inline to its status and message, or propagate it
pub fn form_error(e: scorarium_engine::Error) -> Result<(StatusCode, String)> {
    use scorarium_engine::Error as E;
    match e {
        E::WrongPassword => Ok((StatusCode::UNAUTHORIZED, "Wrong password.".to_owned())),
        E::Throttled { retry_after } => {
            let minutes = retry_after.as_secs().div_ceil(60);
            Ok((
                StatusCode::TOO_MANY_REQUESTS,
                format!("Try again in {minutes} minutes."),
            ))
        }
        E::InvalidValue(message) => Ok((StatusCode::BAD_REQUEST, message)),
        e => Err(e.into()),
    }
}
