pub mod index;

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
