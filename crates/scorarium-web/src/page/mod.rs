pub mod index;

use crate::widget::header::Header;

/// Context for rendering the base.html page template
#[derive(Debug)]
pub struct Base {
    pub header: Header,
    pub title: String,
}

impl Base {
    pub fn new(header: Header, title: impl Into<String>) -> Base {
        Base {
            header,
            title: title.into(),
        }
    }
}
